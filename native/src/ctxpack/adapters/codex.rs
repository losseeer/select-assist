//! Codex CLI：~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl（或 $CODEX_HOME）

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::ctxpack::adapters::util::{self, RawTurn};
use crate::ctxpack::adapters::{
    head_records, mtime_ms, note_dropped, read_whole, sort_by_mtime, unparsable_dropped, Adapter,
    DiscoverOpts, SessionRef, TranscriptResult, EMPTY_FILE,
};

const AGENT: &str = "codex";
const ADAPTER: &str = "codex-jsonl@0";

pub struct Codex;

fn walk_date_dirs(root: &Path, depth: usize, files: &mut Vec<PathBuf>) {
    if depth > 4 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if path.is_dir() {
            walk_date_dirs(&path, depth + 1, files);
        } else if name.starts_with("rollout-") && name.ends_with(".jsonl") {
            files.push(path);
        }
    }
}

/// session_meta 里带 cwd / session_id；预览取第一条用户 input_text
fn meta_of(records: &[Value]) -> (Option<String>, Option<String>, Option<String>) {
    let mut cwd = None;
    let mut session_id = None;
    let mut preview = None;
    for record in records {
        let kind = record
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let payload = record.get("payload");
        if kind == "session_meta" {
            cwd = payload
                .and_then(|p| p.get("cwd"))
                .and_then(Value::as_str)
                .map(str::to_string);
            session_id = payload
                .and_then(|p| p.get("session_id"))
                .and_then(Value::as_str)
                .map(str::to_string);
        } else if preview.is_none()
            && kind == "response_item"
            && payload.and_then(|p| p.get("type")).and_then(Value::as_str) == Some("message")
            && payload.and_then(|p| p.get("role")).and_then(Value::as_str) == Some("user")
        {
            for block in payload
                .and_then(|p| p.get("content"))
                .and_then(Value::as_array)
                .map(|v| v.iter())
                .into_iter()
                .flatten()
            {
                if block.get("type").and_then(Value::as_str) != Some("input_text") {
                    continue;
                }
                let text =
                    util::strip_synthetic(block.get("text").and_then(Value::as_str).unwrap_or(""));
                if !text.is_empty() {
                    preview = Some(util::preview_of(&text));
                    break;
                }
            }
        }
        if cwd.is_some() && session_id.is_some() && preview.is_some() {
            break;
        }
    }
    (cwd, session_id, preview)
}

fn codex_home(home: Option<&str>) -> PathBuf {
    match home {
        Some(home) => PathBuf::from(home).join(".codex"),
        None => std::env::var_os("CODEX_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| dirs::home_dir().expect("no home directory").join(".codex")),
    }
}

impl Adapter for Codex {
    fn discover(&self, opts: &DiscoverOpts) -> Vec<SessionRef> {
        let sessions_root = opts
            .root
            .clone()
            .map(PathBuf::from)
            .unwrap_or_else(|| codex_home(opts.home.as_deref()).join("sessions"));
        let mut files = Vec::new();
        walk_date_dirs(&sessions_root, 0, &mut files);

        let mut refs: Vec<SessionRef> = Vec::new();
        for path in files {
            let file = path.to_string_lossy().to_string();
            let Some(mtime) = mtime_ms(&file) else {
                continue;
            };
            let records = head_records(&file).unwrap_or_default();
            let (cwd, session_id, preview) = meta_of(&records);
            refs.push(SessionRef {
                agent: AGENT.to_string(),
                adapter: ADAPTER.to_string(),
                file_path: file,
                session_id,
                project_path: cwd,
                preview,
                mtime_ms: mtime,
                ..Default::default()
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
            if record.get("type").and_then(Value::as_str) != Some("response_item") {
                continue;
            }
            let Some(payload) = record.get("payload") else {
                continue;
            };
            let kind = payload
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if kind == "message" {
                // developer 指令是系统提示词的内容，永远不进包
                let role = payload
                    .get("role")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if role != "user" && role != "assistant" {
                    continue;
                }
                let blocks = payload
                    .get("content")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                for block in blocks {
                    let block_type = block
                        .get("type")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    if !matches!(block_type, "input_text" | "output_text" | "text") {
                        continue;
                    }
                    let stripped = util::strip_synthetic(
                        block.get("text").and_then(Value::as_str).unwrap_or(""),
                    );
                    if !stripped.is_empty() {
                        raw.push(RawTurn {
                            role: role.to_string(),
                            text: stripped,
                        });
                    }
                }
            } else if kind == "reasoning" {
                note_dropped(&mut dropped, "reasoning");
            } else if matches!(
                kind,
                "function_call"
                    | "function_call_output"
                    | "custom_tool_call"
                    | "custom_tool_call_output"
                    | "web_search_call"
            ) {
                note_dropped(&mut dropped, "tool-results");
            }
        }
        TranscriptResult {
            turns: util::merge_turns(raw),
            dropped,
            error: None,
        }
    }
}
