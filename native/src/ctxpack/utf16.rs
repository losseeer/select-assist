//! JS 的 `string.length` / `slice` 按 UTF-16 码元计数，ctxpack 的裁剪口径全靠它。
//! Rust 的 `chars()` 与之不等价（emoji 等辅助平面字符差 1），所以裁剪一律走这里。

pub fn len(s: &str) -> usize {
    s.encode_utf16().count()
}

/// 等价于 JS 的 `s.slice(from, to)`（下标按 UTF-16 码元）。
/// 切在代理对中间时 JS 会留下孤立代理项，Rust 侧只能替换成 U+FFFD —— 唯一已知差异，
/// 只影响「emoji 正好落在裁剪边界」时的一个字符。
pub fn slice(s: &str, from: usize, to: usize) -> String {
    let units: Vec<u16> = s.encode_utf16().collect();
    let from = from.min(units.len());
    let to = to.max(from).min(units.len());
    String::from_utf16_lossy(&units[from..to])
}

/// `s.slice(0, n)`
pub fn head(s: &str, n: usize) -> String {
    slice(s, 0, n)
}

/// `s.slice(-n)`
pub fn tail(s: &str, n: usize) -> String {
    let total = len(s);
    slice(s, total.saturating_sub(n), total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_utf16_units_like_js() {
        assert_eq!(len("中文"), 2);
        assert_eq!(len("👍"), 2);
        assert_eq!(len("a👍b"), 4);
    }

    #[test]
    fn slices_by_units_not_by_chars() {
        assert_eq!(head("a👍b", 2), "a\u{fffd}"); // 切在代理对中间
        assert_eq!(head("a👍b", 3), "a👍");
        assert_eq!(tail("a👍b", 1), "b");
        assert_eq!(head("abc", 10), "abc");
        assert_eq!(tail("abc", 99), "abc");
    }
}
