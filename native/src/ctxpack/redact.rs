//! 路径脱敏：家目录写成 ~，别人的用户名不留在包里。
//! 对照 packages/ctxpack/src/redact.ts —— 只作用于组装上下文，直通模式的选区不经过这里。

use std::sync::LazyLock;

use regex::Regex;

use crate::ctxpack::types::CtxPack;

/// `/Users/<name>` → `~/user`
static POSIX_USERS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"/Users/[^/\s"]+"#).unwrap());
/// `C:\Users\<name>` → `~\user`
static WINDOWS_USERS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"[A-Za-z]:[\\/]Users[\\/][^\s"\\/]+"#).unwrap());

fn sub(text: &str, home: &str) -> String {
    let folded = if home.is_empty() {
        text.to_string()
    } else {
        text.replace(home, "~")
    };
    let folded = POSIX_USERS.replace_all(&folded, "~/user").to_string();
    WINDOWS_USERS.replace_all(&folded, "~\\user").to_string()
}

pub fn redact_paths(pack: &CtxPack, home: &str) -> CtxPack {
    let mut out = pack.clone();
    if let Some(payload) = &out.payload {
        out.payload = Some(sub(payload, home));
    }
    if let Some(source) = &mut out.source {
        if source
            .project_path
            .as_deref()
            .is_some_and(|p| !p.is_empty())
        {
            source.project_path = Some(sub(source.project_path.as_deref().unwrap(), home));
        }
    }
    if let Some(selection) = &mut out.selection {
        if !selection.text.is_empty() {
            selection.text = sub(&selection.text.clone(), home);
        }
    }
    if let Some(turns) = &mut out.transcript {
        for turn in turns.iter_mut() {
            turn.text = sub(&turn.text.clone(), home);
        }
    }
    out
}
