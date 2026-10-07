//! Qoder 是复合 adapter：CN IDE 的 ~/.qoder-cn/projects（Claude 风格 JSONL）
//! 与 QoderWork 的 agents.db（sqlite）都要看。对照 src/adapters/qoder.ts。

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};
use serde_json::Value;

use crate::ctxpack::adapters::jsonl::{self, home_relative};
use crate::ctxpack::adapters::util::{self, RawTurn};
use crate::ctxpack::adapters::{
    note_dropped, sort_by_mtime, Adapter, DiscoverOpts, SessionRef, TranscriptResult,
};

const AGENT: &str = "qoder";
const COMPOSITE: &str = "qoder-composite@0";
const SQLITE_ADAPTER: &str = "qoderwork-sqlite@0";

pub struct Qoder;

pub struct QoderWork;

pub fn qoder_cn_adapter() -> jsonl::JsonlAdapter {
    jsonl::make(AGENT, "qoder-cn-jsonl@0", |home| {
        home_relative(home, &[".qoder-cn", "projects"])
    })
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Platform {
    Windows,
    MacOS,
    Other,
}

pub fn current_platform() -> Platform {
    match std::env::consts::OS {
        "windows" => Platform::Windows,
        "macos" => Platform::MacOS,
        _ => Platform::Other,
    }
}

fn user_home() -> PathBuf {
    dirs::home_dir().expect("no home directory")
}

/// QoderWork 是 Electron 应用，userData 目录按平台走；传 `home` 时恒等于 macOS 布局（fixture 就是这么建的）
fn support_dir(
    home: Option<&str>,
    platform: Platform,
    lookup: &mut dyn FnMut(&str) -> Option<String>,
) -> PathBuf {
    if let Some(home) = home {
        return PathBuf::from(home).join("Library/Application Support");
    }
    match platform {
        Platform::Windows => lookup("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| user_home().join("AppData/Roaming")),
        Platform::MacOS => user_home().join("Library/Application Support"),
        Platform::Other => lookup("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| user_home().join(".config")),
    }
}

fn env_lookup(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|value| !value.is_empty())
}

pub fn qoder_work_db_path(home: Option<&str>) -> PathBuf {
    support_dir(home, current_platform(), &mut env_lookup)
        .join("QoderWork")
        .join("data")
        .join("agents.db")
}

/// 时间戳有秒也有毫秒两种存法
fn to_ms(value: Option<i64>) -> f64 {
    let n = value.unwrap_or(0) as f64;
    if n > 1e12 {
        n
    } else {
        n * 1000.0
    }
}

/// 第一个能读的 text 段；其余算非文本
fn first_text(parts_json: Option<&str>) -> Option<String> {
    let Value::Array(parts) = serde_json::from_str(parts_json?).ok()? else {
        return None;
    };
    parts.iter().find_map(|part| {
        if part.get("type").and_then(Value::as_str) != Some("text") {
            return None;
        }
        let stripped =
            util::strip_synthetic(part.get("text").and_then(Value::as_str).unwrap_or(""));
        (!stripped.is_empty()).then_some(stripped)
    })
}

fn open_readonly(file: &str) -> Option<Connection> {
    if !Path::new(file).exists() {
        return None;
    }
    Connection::open_with_flags(
        file,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .ok()
}

impl Adapter for QoderWork {
    fn agent(&self) -> &str {
        AGENT
    }

    fn adapter(&self) -> &str {
        SQLITE_ADAPTER
    }

    fn discover(&self, opts: &DiscoverOpts) -> Vec<SessionRef> {
        let file = opts.root.clone().unwrap_or_else(|| {
            qoder_work_db_path(opts.home.as_deref())
                .to_string_lossy()
                .to_string()
        });
        let Some(db) = open_readonly(&file) else {
            return Vec::new();
        };
        let Ok(mut statement) = db.prepare(
            "select c.id, c.name, c.updated_at, p.path as project_path
               from chats c left join projects p on p.id = c.project_id
              where c.deleted_at is null
              order by c.updated_at desc limit 60",
        ) else {
            return Vec::new();
        };
        let chats = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            })
            .map(|rows| rows.filter_map(Result::ok).collect::<Vec<_>>())
            .unwrap_or_default();

        let mut preview_statement =
            db.prepare("select parts from messages where chat_id = ? and role = 'user' order by sequence limit 1")
                .ok();
        let mut refs = Vec::new();
        for (id, name, updated_at, project_path) in chats {
            // 预览是尽力而为
            let preview = match &mut preview_statement {
                Some(statement) => statement
                    .query_row([id.as_str()], |row| row.get::<_, Option<String>>(0))
                    .ok()
                    .flatten()
                    .and_then(|parts| first_text(Some(&parts)))
                    .map(|text| util::preview_of(&text)),
                None => None,
            };
            refs.push(SessionRef {
                agent: AGENT.to_string(),
                adapter: SQLITE_ADAPTER.to_string(),
                file_path: file.clone(),
                session_id: Some(id),
                name,
                project_path,
                preview,
                mtime_ms: to_ms(updated_at),
            });
        }
        util::by_cwd(refs, opts.cwd.as_deref(), opts.limit())
    }

    fn read_transcript(&self, reference: &SessionRef) -> TranscriptResult {
        let Some(db) = open_readonly(&reference.file_path) else {
            return TranscriptResult {
                error: Some(format!("无法打开 Qoder 数据库: {}", reference.file_path)),
                ..Default::default()
            };
        };
        let Ok(mut statement) = db.prepare(
            "select role, parts from messages where chat_id = ? order by sub_chat_id, sequence",
        ) else {
            return TranscriptResult {
                error: Some("无法打开 Qoder 数据库".to_string()),
                ..Default::default()
            };
        };
        let rows = statement
            .query_map([reference.session_id.clone().unwrap_or_default()], |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                ))
            })
            .map(|rows| rows.filter_map(Result::ok).collect::<Vec<_>>())
            .unwrap_or_default();
        if rows.is_empty() {
            return TranscriptResult {
                error: Some("该会话没有消息".to_string()),
                ..Default::default()
            };
        }

        let mut dropped: Vec<String> = Vec::new();
        let mut raw: Vec<RawTurn> = Vec::new();
        for (role, parts_json) in rows {
            let role = role.unwrap_or_default();
            if role != "user" && role != "assistant" {
                continue;
            }
            let Ok(Value::Array(parts)) =
                serde_json::from_str::<Value>(&parts_json.unwrap_or_else(|| "[]".to_string()))
            else {
                note_dropped(&mut dropped, "unparsable-parts");
                continue;
            };
            for part in parts {
                let kind = part.get("type").and_then(Value::as_str).unwrap_or_default();
                if kind == "text" {
                    let text = part.get("text").and_then(Value::as_str).unwrap_or("");
                    let stripped = util::strip_synthetic(text);
                    if !stripped.is_empty() {
                        raw.push(RawTurn {
                            role: role.to_string(),
                            text: stripped,
                        });
                    }
                } else if kind == "error" {
                    note_dropped(&mut dropped, "errors");
                } else {
                    note_dropped(&mut dropped, "tool-parts");
                }
            }
        }
        TranscriptResult {
            turns: util::merge_turns(raw),
            dropped,
            error: None,
        }
    }
}

impl Adapter for Qoder {
    fn agent(&self) -> &str {
        AGENT
    }

    fn adapter(&self) -> &str {
        COMPOSITE
    }

    fn discover(&self, opts: &DiscoverOpts) -> Vec<SessionRef> {
        let limit = opts.limit();
        // 显式 root 只指向两个 qoder 存储里的某一个
        if let Some(root) = &opts.root {
            return if root.ends_with(".db") {
                QoderWork.discover(&DiscoverOpts {
                    limit,
                    ..opts.clone()
                })
            } else {
                qoder_cn_adapter().discover(&DiscoverOpts {
                    limit,
                    ..opts.clone()
                })
            };
        }
        let mut refs = qoder_cn_adapter().discover(&DiscoverOpts {
            limit,
            ..opts.clone()
        });
        refs.extend(QoderWork.discover(&DiscoverOpts {
            limit,
            ..opts.clone()
        }));
        sort_by_mtime(&mut refs);
        refs.truncate(limit);
        refs
    }

    fn read_transcript(&self, reference: &SessionRef) -> TranscriptResult {
        if reference.file_path.ends_with(".jsonl") {
            qoder_cn_adapter().read_transcript(reference)
        } else {
            QoderWork.read_transcript(reference)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_uses_appdata_and_posix_uses_xdg() {
        let env = |key: &str| (key == "APPDATA").then(|| "D:/AppData".to_string());
        let mut lookup = |k: &str| env(k);
        assert_eq!(
            support_dir(None, Platform::Windows, &mut lookup),
            PathBuf::from("D:/AppData")
        );
        // 没有 APPDATA 才退回用户目录
        let mut empty = |_k: &str| None;
        assert_eq!(
            support_dir(Some("/h"), Platform::Windows, &mut empty),
            PathBuf::from("/h/Library/Application Support")
        );
        assert!(
            support_dir(None, Platform::MacOS, &mut empty).ends_with("Library/Application Support")
        );
    }

    #[test]
    fn ms_and_seconds_timestamps_both_land_on_ms() {
        assert_eq!(to_ms(Some(1_700_000_000)), 1_700_000_000_000.0);
        assert_eq!(to_ms(Some(1_700_000_000_000)), 1_700_000_000_000.0);
        assert_eq!(to_ms(None), 0.0);
    }

    #[test]
    fn first_text_skips_non_text_parts() {
        let parts = r#"[{"type":"tool-Thinking","input":{"text":"PRIVATE"}},{"type":"text","text":"好的，第一个问题："}]"#;
        assert_eq!(
            first_text(Some(parts)).as_deref(),
            Some("好的，第一个问题：")
        );
        assert_eq!(first_text(None), None);
        assert_eq!(first_text(Some("not json")), None);
    }
}
