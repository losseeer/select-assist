//! Claude Code：~/.claude/projects/<project>/<uuid>.jsonl

use crate::ctxpack::adapters::jsonl::{self, JsonlAdapter};

pub fn adapter() -> JsonlAdapter {
    jsonl::make("claude-code", "claude-code-jsonl@0", |home| {
        jsonl::home_relative(home, &[".claude", "projects"])
    })
}
