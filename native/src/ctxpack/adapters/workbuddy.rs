//! WorkBuddy：~/.workbuddy/projects/<project>/<uuid>.jsonl，记录形如 {type:'message',role,content}

use std::path::PathBuf;

use serde_json::Value;

use crate::ctxpack::adapters::jsonl::home_relative;
use crate::ctxpack::adapters::util::{self, RawTurn};
use crate::ctxpack::adapters::{
    head_records, mtime_ms, note_dropped, read_whole, sort_by_mtime, unparsable_dropped, Adapter,
    DiscoverOpts, SessionRef, TranscriptResult, EMPTY_FILE,
};

const AGENT: &str = "workbuddy";
const ADAPTER: &str = "workbuddy-jsonl@0";

pub struct Workbuddy;

fn str_field<'a>(record: &'a Value, key: &str) -> Option<&'a str> {
    record.get(key).and_then(Value::as_str)
}

fn texts_of(record: &Value) -> Vec<String> {
    let blocks = match record.get("content") {
        Some(Value::String(text)) => vec![serde_json::json!({ "type": "text", "text": text })],
        Some(Value::Array(blocks)) => blocks.clone(),
        _ => Vec::new(),
    };
    blocks
        .iter()
        .filter(|b| {
            matches!(
                b.get("type").and_then(Value::as_str),
                Some("input_text") | Some("output_text") | Some("text")
            )
        })
        .map(|b| util::strip_synthetic(b.get("text").and_then(Value::as_str).unwrap_or("")))
        .filter(|t| !t.is_empty())
        .collect()
}

impl Adapter for Workbuddy {
    fn discover(&self, opts: &DiscoverOpts) -> Vec<SessionRef> {
        let projects_dir = opts.root.clone().unwrap_or_else(|| {
            home_relative(opts.home.as_deref(), &[".workbuddy", "projects"])
                .to_string_lossy()
                .to_string()
        });
        let mut refs: Vec<SessionRef> = Vec::new();
        let Ok(entries) = std::fs::read_dir(&projects_dir) else {
            return refs;
        };
        for entry in entries.flatten() {
            let dir = entry.path();
            let Ok(files) = std::fs::read_dir(&dir) else {
                continue;
            };
            for file in files.flatten() {
                let path: PathBuf = file.path();
                let name = path.to_string_lossy().to_string();
                if !name.ends_with(".jsonl") {
                    continue;
                }
                let Some(mtime) = mtime_ms(&name) else {
                    continue;
                };
                let records = head_records(&name).unwrap_or_default();
                let mut project_path: Option<String> = None;
                let mut preview: Option<String> = None;
                for record in &records {
                    if project_path.is_none() {
                        project_path = str_field(record, "cwd").map(str::to_string);
                    }
                    if preview.is_none()
                        && str_field(record, "type") == Some("message")
                        && str_field(record, "role") == Some("user")
                    {
                        preview = texts_of(record).first().map(|t| util::preview_of(t));
                    }
                    if project_path.is_some() && preview.is_some() {
                        break;
                    }
                }
                let base = path
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default();
                refs.push(SessionRef {
                    agent: AGENT.to_string(),
                    adapter: ADAPTER.to_string(),
                    file_path: name,
                    session_id: util::uuid_like(&base),
                    project_path,
                    preview,
                    mtime_ms: mtime,
                    ..Default::default()
                });
            }
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
            let kind = str_field(record, "type").unwrap_or_default();
            let role = str_field(record, "role").unwrap_or_default();
            if kind == "message" && (role == "user" || role == "assistant") {
                for t in texts_of(record) {
                    raw.push(RawTurn {
                        role: role.to_string(),
                        text: t,
                    });
                }
            } else if kind == "reasoning" {
                note_dropped(&mut dropped, "reasoning");
            } else if matches!(
                kind,
                "function_call" | "function_call_result" | "tool-call" | "tool-result"
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
