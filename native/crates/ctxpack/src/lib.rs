//! ctxpack 的 Rust 直译：行为规格是 packages/ctxpack/test/ 里的 36 个用例，
//! fixture 已复制到 native/fixtures/，输出必须逐字一致。

pub mod adapters;
pub mod build;
pub mod prompt;
pub mod redact;
pub mod render;

pub mod time;
pub mod types;
pub mod utf16;
// 契约校验器只有规格测试在用（Electron 侧同理），不进交付二进制
#[cfg(test)]
pub mod validate;

#[cfg(test)]
mod tests;

// 只转出真正被面板用到的名字；其余走 crate::<module>::<item> 原路径
pub use adapters::SessionRef;
pub use build::build_pack;
pub use redact::redact_paths;
pub use render::render;

use crate::adapters::util::match_cwd;

/// 会话归属：先精确 cwd，再前缀匹配，最后按 mtime 猜 —— 猜的必须说明是猜的
pub fn pick_session(refs: &[SessionRef], cwd: Option<&str>) -> Option<(SessionRef, String)> {
    let first = refs.first()?;
    if let Some(cwd) = cwd {
        let mut under: Option<&SessionRef> = None;
        for reference in refs {
            match match_cwd(reference.project_path.as_deref(), Some(cwd)) {
                Some("exact") => {
                    return Some((
                        reference.clone(),
                        format!(
                            "cwd 精确匹配 {}",
                            reference.project_path.clone().unwrap_or_default()
                        ),
                    ))
                }
                Some("under") if under.is_none() => under = Some(reference), // JS 的 under ??= ref
                _ => {}
            }
        }
        if let Some(under) = under {
            return Some((
                under.clone(),
                format!(
                    "cwd 前缀匹配 {}",
                    under.project_path.clone().unwrap_or_default()
                ),
            ));
        }
    }
    let basis = match cwd {
        Some(cwd) => format!("未匹配 cwd（{cwd}），按最近修改时间猜测"),
        None => "未提供项目路径，按最近修改时间猜测".to_string(),
    };
    Some((first.clone(), basis))
}
