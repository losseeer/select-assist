//! Windows 原生外壳。mac 侧对应 crates/app-mac。
//!
//! M0 只做一件事：把「不抢焦点、常驻顶层、400×44 无边框」这块窗口骨架立起来，
//! 因为后面无论内容用 Direct2D 手画还是 WebView2 承载，都要求同一套窗口语义。
//! 非 Windows 平台上这个 crate 编成空 main，保证 Mac 上的 `cargo test` 不会多一个编不过的目标。

#[cfg(windows)]
mod shell;
#[cfg(windows)]
fn main() -> windows::core::Result<()> {
    shell::run()
}

#[cfg(not(windows))]
fn main() {}
