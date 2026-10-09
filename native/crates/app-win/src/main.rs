//! Windows 原生外壳。mac 侧对应 crates/app-mac。
//!
//! 三个文件分工：shell.rs 是窗口、消息与状态；paint.rs / panel.rs 是纯视图；
//! edits.rs 管设置组那三个原生文本框。
//! 非 Windows 平台上这个 crate 编成空 main，保证 Mac 上的 `cargo test` 不会多一个编不过的目标。

#[cfg(windows)]
mod clip;
#[cfg(windows)]
mod draw;
#[cfg(windows)]
mod edits;
#[cfg(windows)]
mod paint;
#[cfg(windows)]
mod panel;
#[cfg(windows)]
mod shell;
#[cfg(windows)]
mod theme;
#[cfg(windows)]
fn main() -> windows::core::Result<()> {
    shell::run()
}

#[cfg(not(windows))]
fn main() {}
