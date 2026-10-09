//! 选区模型 + 状态文案。M2 只有直通这一条路径：取进来什么，复制回去就是什么。
//! 逐条对齐 packages/panel/src/main/capture.ts 与 static/renderer.js 的口径。

use ctxpack::utf16;

/// 已取入的选区。`text` 是 trim 之后的原文，直通模式逐字节写回它（不套模板、不脱敏）。
#[derive(Clone, Debug, PartialEq)]
pub struct Selection {
    pub text: String,
    pub first_line: String,
    /// JS 的 text.length，即 UTF-16 码元数
    pub chars: usize,
    /// 本地时间 HH:MM:SS，只用于 tooltip
    pub at: String,
}

/// 未读小红点携带的信息（对应 Electron 的 clipUnread）
#[derive(Clone, Debug, PartialEq)]
pub struct ClipNote {
    pub chars: usize,
    pub first_line: String,
}

pub const EMPTY_REASON: &str = "剪贴板为空，请先在源界面复制选中的文本";
pub const NO_SELECTION: &str = "还没有选区";
pub const NO_SELECTION_TIP: &str = "在源界面复制，再点「取入选区」";
pub const CAPTURED: &str = "已取入选区";
pub const CONTEXT_EMPTY: &str = "上下文未填充";

/// JS 的 `text.split('\n')[0]?.slice(0, limit)`：下标按 UTF-16 码元，与 utf16.rs 同一口径
fn first_line(text: &str, limit: usize) -> String {
    utf16::head(text.split('\n').next().unwrap_or(""), limit)
}

/// captureFromClipboard：空白剪贴板（例如复制了一个空行）不算取入，也不能让它把空白当选区存下
pub fn from_clipboard(text: &str, at: &str) -> Result<Selection, String> {
    if text.trim().is_empty() {
        return Err(EMPTY_REASON.to_string());
    }
    Ok(Selection {
        text: text.trim().to_string(),
        first_line: first_line(text, 120),
        chars: utf16::len(text),
        at: at.to_string(),
    })
}

pub fn note_from(text: &str) -> ClipNote {
    ClipNote {
        chars: utf16::len(text),
        first_line: first_line(text, 80),
    }
}

/// 状态行：首行为空时退回「已取入选区」，没有选区就说还没有
pub fn status_text(selection: Option<&Selection>) -> String {
    match selection {
        Some(s) if s.first_line.is_empty() => CAPTURED.to_string(),
        Some(s) => s.first_line.clone(),
        None => NO_SELECTION.to_string(),
    }
}

pub fn status_tip(selection: Option<&Selection>) -> String {
    match selection {
        Some(s) => format!("来源：剪贴板 · {} · {} 字", s.at, s.chars),
        None => NO_SELECTION_TIP.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stores_the_trimmed_text_but_counts_the_raw_one() {
        let s = from_clipboard("  你好\n世界  ", "10:20:30").unwrap();
        assert_eq!(s.text, "你好\n世界");
        assert_eq!(s.chars, 9); // JS 的 length 含两端空格与换行
        assert_eq!(s.first_line, "  你好"); // 首行取的是未 trim 的原文，和 Electron 一致
        assert_eq!(s.at, "10:20:30");
    }

    #[test]
    fn whitespace_only_clipboard_is_rejected() {
        assert_eq!(
            from_clipboard(" \n\t ", "00:00:00"),
            Err(EMPTY_REASON.to_string())
        );
    }

    #[test]
    fn emoji_count_the_same_way_as_js_length() {
        assert_eq!(utf16::len("👍"), 2);
        assert_eq!(utf16::len("中文"), 2);
    }

    #[test]
    fn first_line_is_capped_for_display() {
        let long = "x".repeat(200);
        assert_eq!(note_from(&long).first_line.chars().count(), 80);
        assert_eq!(
            from_clipboard(&long, "0")
                .unwrap()
                .first_line
                .chars()
                .count(),
            120
        );
        // JS 用 slice()，上限数的是 UTF-16 码元而不是字符：一个 emoji 占两格
        let emoji = format!("{}yz", "👍".repeat(70));
        assert_eq!(note_from(&emoji).first_line, "👍".repeat(40));
        assert_eq!(from_clipboard(&emoji, "0").unwrap().chars, 142);
    }

    #[test]
    fn status_line_follows_the_model() {
        assert_eq!(status_text(None), NO_SELECTION);
        assert_eq!(status_tip(None), NO_SELECTION_TIP);

        let s = from_clipboard("abc", "09:00:00").unwrap();
        assert_eq!(status_text(Some(&s)), "abc");
        assert_eq!(status_tip(Some(&s)), "来源：剪贴板 · 09:00:00 · 3 字");

        // 首行是空的情况（选中内容以换行开头）
        let blank_first = Selection {
            text: "x".into(),
            first_line: String::new(),
            chars: 1,
            at: "0".into(),
        };
        assert_eq!(status_text(Some(&blank_first)), CAPTURED);
    }
}
