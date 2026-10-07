//! adapter 共享工具，对照 packages/ctxpack/src/adapters/util.ts。

use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

use crate::ctxpack::adapters::SessionRef;
use crate::ctxpack::utf16;

/// harness 会往用户消息里塞合成块（IDE 上下文、提醒、slash 命令包装），这些不是用户打的字。
/// DROP 整块删掉；UNWRAP 只脱标签、留下真正的内容。
const DROP_TAGS: [&str; 13] = [
    "system-reminder",
    "ide_opened_file",
    "ide_selection",
    "environment_context",
    "command-name",
    "command-message",
    "command-args",
    "local-command-stdout",
    "local-command-caveat",
    "user-prompt-submit-hook",
    "skills_instructions",
    "collaboration_mode",
    "permissions",
];
const UNWRAP_TAGS: [&str; 2] = ["user_query", "user_message"];

fn regex(pattern: &str) -> Regex {
    Regex::new(pattern).expect("固定的 tag 名，模式不会失配")
}

/// 每个 tag 两条：`<tag ...> ... </tag ...>` 非贪婪配对，以及 `<tag .../>` 自闭合
static DROP_PAIRS: LazyLock<Vec<(Regex, Regex)>> = LazyLock::new(|| {
    DROP_TAGS
        .iter()
        .map(|tag| {
            (
                regex(&format!("<{tag}[^>]*>[\\s\\S]*?</{tag}[^>]*>")),
                regex(&format!("<{tag}[^>]*/>")),
            )
        })
        .collect()
});
static UNWRAPS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    UNWRAP_TAGS
        .iter()
        .map(|tag| regex(&format!("<{tag}[^>]*>([\\s\\S]*?)</{tag}[^>]*>")))
        .collect()
});

pub fn strip_synthetic(text: &str) -> String {
    let mut out = text.to_string();
    for (pair, alone) in DROP_PAIRS.iter() {
        out = pair.replace_all(&out, "").to_string();
        out = alone.replace_all(&out, "").to_string();
    }
    for pattern in UNWRAPS.iter() {
        out = pattern.replace_all(&out, "$1").to_string();
    }
    out.trim().to_string()
}

/// JSONL：坏行只计数，不让整个文件读不出来
pub fn parse_json_lines(text: &str) -> (Vec<Value>, usize) {
    let mut records = Vec::new();
    let mut bad = 0;
    for line in text.split('\n') {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        match serde_json::from_str(trimmed) {
            Ok(value) => records.push(value),
            Err(_) => bad += 1,
        }
    }
    (records, bad)
}

pub struct RawTurn {
    pub role: String,
    pub text: String,
}

/// 同一角色连续的多条合成一条（seq 就是最终下标）
pub fn merge_turns(raw: Vec<RawTurn>) -> Vec<crate::ctxpack::types::TranscriptTurn> {
    let mut merged: Vec<crate::ctxpack::types::TranscriptTurn> = Vec::new();
    for turn in raw {
        match merged.last_mut() {
            Some(last) if last.role == turn.role => {
                last.text = format!("{}\n{}", last.text, turn.text)
            }
            _ => merged.push(crate::ctxpack::types::TranscriptTurn {
                role: turn.role,
                text: turn.text,
                seq: Some(merged.len()),
            }),
        }
    }
    merged
}

fn canon(path: &str) -> String {
    let mut s = path.replace('\\', "/");
    if s.chars().count() > 1 {
        while s.ends_with('/') {
            s.pop();
        }
    }
    // 盘符决定要不要折叠大小写：POSIX 路径永远保持区分
    let drive_prefixed = {
        let bytes = s.as_bytes();
        bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'/'
    };
    if drive_prefixed {
        s = s.to_lowercase();
    }
    s
}

/// "exact" | "under" | None
pub fn match_cwd(project_path: Option<&str>, cwd: Option<&str>) -> Option<&'static str> {
    let (Some(project_path), Some(cwd)) = (project_path, cwd) else {
        return None;
    };
    let project = canon(project_path);
    let wanted = canon(cwd);
    if project == wanted {
        return Some("exact");
    }
    if project.starts_with(&format!("{wanted}/")) {
        return Some("under");
    }
    None
}

/// 发现保持宽松：项目被移动或改名之后，旧会话文件仍要能冒出来，所以匹配不到就退回全量
pub fn by_cwd(refs: Vec<SessionRef>, cwd: Option<&str>, limit: usize) -> Vec<SessionRef> {
    let matched: Vec<SessionRef> = match cwd {
        Some(cwd) => refs
            .iter()
            .filter(|r| match_cwd(r.project_path.as_deref(), Some(cwd)).is_some())
            .cloned()
            .collect(),
        None => refs.clone(),
    };
    let mut out = if matched.is_empty() { refs } else { matched };
    out.truncate(limit);
    out
}

pub fn preview_of(text: &str) -> String {
    utf16::head(text, 160)
}

/// UUID 形状的文件名才当 sessionId 用
pub fn uuid_like(base: &str) -> Option<String> {
    (base.len() == 36
        && base
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b) || b == b'-'))
    .then(|| base.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_synthetic_blocks_and_unwraps_queries() {
        let raw =
            "<system-reminder data-role=\"user-context\">ENV MUST NOT APPEAR</system-reminder>\
                   <user_query>这个 JSON 够不够？</user_query>";
        assert_eq!(strip_synthetic(raw), "这个 JSON 够不够？");
        assert_eq!(strip_synthetic("<ide_opened_file/>后"), "后");
        assert_eq!(strip_synthetic("没标签"), "没标签");
    }

    #[test]
    fn merge_turns_folds_same_role_and_numbers_seq() {
        let merged = merge_turns(vec![
            RawTurn {
                role: "user".into(),
                text: "a".into(),
            },
            RawTurn {
                role: "user".into(),
                text: "b".into(),
            },
            RawTurn {
                role: "assistant".into(),
                text: "c".into(),
            },
        ]);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].text, "a\nb");
        assert_eq!(merged[0].seq, Some(0));
        assert_eq!(merged[1].seq, Some(1));
    }

    #[test]
    fn bad_lines_are_counted_not_fatal() {
        let (records, bad) = parse_json_lines("{\"a\":1}\nnot json\n\n{\"b\":2}\n");
        assert_eq!(records.len(), 2);
        assert_eq!(bad, 1);
    }

    #[test]
    fn uuid_like_only_accepts_36_hex_dash_chars() {
        assert!(uuid_like("11111111-2222-3333-4444-555555555555").is_some());
        assert!(uuid_like("deadbeef").is_none());
        assert!(uuid_like(&"g".repeat(36)).is_none());
    }
}
