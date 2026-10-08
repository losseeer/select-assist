//! Claude-Code 风格 JSONL 的通用实现：claude-code 与 qoder-cn 共用（TS 侧同样是 makeJsonlAdapter）。

use std::path::PathBuf;

use serde_json::Value;

use crate::adapters::util::{self, RawTurn};
use crate::adapters::{
    head_records, mtime_ms, note_dropped, read_whole, sort_by_mtime, unparsable_dropped, Adapter,
    DiscoverOpts, SessionRef, TranscriptResult, EMPTY_FILE,
};

/// harness 自动注入的「会话续接摘要」，是 agent 的管道不是用户的话
const COMPACT_SUMMARY: &str = "This session is being continued from a previous conversation";

fn str_field<'a>(record: &'a Value, key: &str) -> Option<&'a str> {
    record.get(key).and_then(Value::as_str)
}

fn blocks_of(record: &Value) -> Vec<Value> {
    let content = record.get("message").and_then(|m| m.get("content"));
    match content {
        Some(Value::String(text)) => vec![serde_json::json!({ "type": "text", "text": text })],
        Some(Value::Array(blocks)) => blocks.clone(),
        _ => Vec::new(),
    }
}

fn cwd_of(records: &[Value]) -> Option<String> {
    records
        .iter()
        .find_map(|r| str_field(r, "cwd"))
        .map(str::to_string)
}

fn first_user_preview(records: &[Value]) -> Option<String> {
    for record in records {
        if str_field(record, "type") != Some("user")
            || record.get("isSidechain") == Some(&Value::Bool(true))
        {
            continue;
        }
        for block in blocks_of(record) {
            if block.get("type").and_then(Value::as_str) != Some("text") {
                continue;
            }
            let raw = block
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let text = util::strip_synthetic(raw);
            if !text.is_empty() && !text.starts_with(COMPACT_SUMMARY) {
                return Some(util::preview_of(&text));
            }
        }
    }
    None
}

fn list_session_files(projects_dir: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(projects_dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let dir = entry.path();
        let Ok(files) = std::fs::read_dir(&dir) else {
            continue;
        }; // 单个文件条目会失败，和 TS 的 try/catch 一样
        for file in files.flatten() {
            let path = file.path();
            if path.to_string_lossy().ends_with(".jsonl") {
                out.push(path);
            }
        }
    }
    out
}

pub struct JsonlAdapter {
    pub agent_name: &'static str,
    pub adapter_id: &'static str,
    pub projects_dir: fn(home: Option<&str>) -> PathBuf,
}

impl Adapter for JsonlAdapter {
    fn discover(&self, opts: &DiscoverOpts) -> Vec<SessionRef> {
        let projects_dir = opts.root.clone().unwrap_or_else(|| {
            (self.projects_dir)(opts.home.as_deref())
                .to_string_lossy()
                .to_string()
        });
        let mut refs: Vec<SessionRef> = Vec::new();
        for path in list_session_files(&projects_dir) {
            let file = path.to_string_lossy().to_string();
            let Some(mtime) = mtime_ms(&file) else {
                continue;
            };
            let records = head_records(&file).unwrap_or_default();
            let base = path
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            refs.push(SessionRef {
                agent: self.agent_name.to_string(),
                adapter: self.adapter_id.to_string(),
                file_path: file,
                session_id: util::uuid_like(&base),
                name: None,
                project_path: cwd_of(&records),
                preview: first_user_preview(&records),
                mtime_ms: mtime,
            });
        }
        sort_by_mtime(&mut refs);
        util::by_cwd(refs, opts.cwd.as_deref(), opts.limit())
    }

    fn read_transcript(&self, reference: &SessionRef) -> TranscriptResult {
        let text = match read_whole(&reference.file_path) {
            Ok(text) => text,
            Err(error) => {
                return TranscriptResult {
                    error: Some(error),
                    ..Default::default()
                }
            }
        };
        let (records, bad) = util::parse_json_lines(&text);
        if records.is_empty() {
            return TranscriptResult {
                error: Some(EMPTY_FILE.to_string()),
                ..Default::default()
            };
        }
        let mut dropped: Vec<String> = Vec::new();
        if let Some(line) = unparsable_dropped(bad) {
            dropped.push(line);
        }

        let mut raw: Vec<RawTurn> = Vec::new();
        for record in &records {
            // 子 agent 的 sidechain 不属于这段对话的表面文本
            if record.get("isSidechain") == Some(&Value::Bool(true)) {
                note_dropped(&mut dropped, "sidechain-turns");
                continue;
            }
            let kind = str_field(record, "type").unwrap_or_default();
            for block in blocks_of(record) {
                let block_type = block
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                match (kind, block_type) {
                    ("user", "text") => {
                        let stripped = util::strip_synthetic(
                            block.get("text").and_then(Value::as_str).unwrap_or(""),
                        );
                        if stripped.is_empty() {
                            continue;
                        }
                        if stripped.starts_with(COMPACT_SUMMARY) {
                            note_dropped(&mut dropped, "compact-summary");
                            continue;
                        }
                        raw.push(RawTurn {
                            role: "user".into(),
                            text: stripped,
                        });
                    }
                    ("user", "tool_result") => note_dropped(&mut dropped, "tool-results"),
                    ("assistant", "text") => {
                        let value = block.get("text").and_then(Value::as_str).unwrap_or("");
                        if !value.trim().is_empty() {
                            raw.push(RawTurn {
                                role: "assistant".into(),
                                text: value.trim().to_string(),
                            });
                        }
                    }
                    ("assistant", "thinking") | ("assistant", "redacted_thinking") => {
                        note_dropped(&mut dropped, "assistant-thinking")
                    }
                    ("assistant", "tool_use") => note_dropped(&mut dropped, "tool-calls"),
                    _ => {}
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

/// 只换目录与 id，其余行为 claude-code / qoder-cn 完全一致（TS 侧是同一个 makeJsonlAdapter）
pub fn make(
    agent_name: &'static str,
    adapter_id: &'static str,
    projects_dir: fn(home: Option<&str>) -> PathBuf,
) -> JsonlAdapter {
    JsonlAdapter {
        agent_name,
        adapter_id,
        projects_dir,
    }
}

pub fn home_relative(home: Option<&str>, parts: &[&str]) -> PathBuf {
    let mut path = home
        .map(PathBuf::from)
        .unwrap_or_else(|| dirs::home_dir().expect("no home directory"));
    for part in parts {
        path = path.join(part);
    }
    path
}
