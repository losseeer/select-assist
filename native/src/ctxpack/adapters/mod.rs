//! Track B adapter 契约，对照 packages/ctxpack/src/adapters/types.ts。

pub mod claude_code;
pub mod codex;
pub mod jsonl;
pub mod qoder;
#[cfg(test)]
mod tests;
pub mod util;
pub mod workbuddy;

use crate::ctxpack::types::TranscriptTurn;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SessionRef {
    pub agent: String,
    pub adapter: String,
    pub file_path: String,
    pub session_id: Option<String>,
    /// agent 自己存了标题时用它（例如 Qoder 的会话名）
    pub name: Option<String>,
    /// 会话文件里记录的 cwd
    pub project_path: Option<String>,
    /// 首条用户消息摘要，给会话浏览器用
    pub preview: Option<String>,
    pub mtime_ms: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TranscriptResult {
    pub turns: Vec<TranscriptTurn>,
    /// 与体积无关、出于隐私默认就丢的东西，例如 "tool-results"
    pub dropped: Vec<String>,
    /// adapter 失败时给出原因，调用方必须降级成只带选区
    pub error: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct DiscoverOpts {
    pub cwd: Option<String>,
    pub limit: usize,
    /// 测试钩子：整棵目录树的根
    pub home: Option<String>,
    /// 设置里指定的扫描位置，优先于内置默认
    pub root: Option<String>,
}

impl DiscoverOpts {
    pub fn limit(&self) -> usize {
        if self.limit == 0 {
            20
        } else {
            self.limit
        }
    }
}

pub trait Adapter {
    /// 新→旧；给了 cwd 就按 cwd 收窄，root 覆盖内置扫描位置
    fn discover(&self, opts: &DiscoverOpts) -> Vec<SessionRef>;
    fn read_transcript(&self, reference: &SessionRef) -> TranscriptResult;
}

pub(crate) fn sort_by_mtime(refs: &mut [SessionRef]) {
    refs.sort_by(|a, b| b.mtime_ms.total_cmp(&a.mtime_ms));
}

pub(crate) fn mtime_ms(path: &str) -> Option<f64> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() {
        return None;
    }
    let since = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?;
    Some(since.as_secs_f64() * 1000.0)
}

/// 只读文件开头 64KB：发现阶段不该为一个大文件付出全量解析
pub(crate) const HEAD_BYTES: usize = 64 * 1024;

pub(crate) fn head_records(file: &str) -> Option<Vec<serde_json::Value>> {
    use std::io::Read;
    let mut handle = std::fs::File::open(file).ok()?;
    let mut buffer = vec![0u8; HEAD_BYTES];
    let read = handle.read(&mut buffer).ok()?;
    let text = String::from_utf8_lossy(&buffer[..read]).to_string();
    Some(util::parse_json_lines(&text).0)
}

pub(crate) fn read_whole(file: &str) -> Result<String, String> {
    std::fs::read_to_string(file).map_err(|e| format!("无法读取会话文件: {e}"))
}

/// 会话文件为空或全部无法解析
pub(crate) const EMPTY_FILE: &str = "会话文件为空或全部无法解析";

/// 同一个理由只记一次
pub(crate) fn note_dropped(dropped: &mut Vec<String>, name: &str) {
    if !dropped.iter().any(|d| d == name) {
        dropped.push(name.to_string());
    }
}

pub(crate) fn unparsable_dropped(bad: usize) -> Option<String> {
    (bad > 0).then(|| format!("unparsable-lines:{bad}"))
}
