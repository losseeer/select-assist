//! Win32 剪贴板。语义对齐 mac 侧的 pasteboard.rs：读文本、写文本、以及一个「变了几次」的
//! 计数（NSPasteboard.changeCount 的对应物是 GetClipboardSequenceNumber）。
//!
//! WM_CLIPBOARDUPDATE 只能由消息循环收到，所以 watch() 把监听器挂到外壳窗口上，
//! shell.rs 在 wndproc 里转成 on_clipboard_update()。

use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL, HWND};
use windows::Win32::System::DataExchange::{
    AddClipboardFormatListener, CloseClipboard, EmptyClipboard, GetClipboardData,
    IsClipboardFormatAvailable, OpenClipboard, RemoveClipboardFormatListener, SetClipboardData,
};
use windows::Win32::System::Memory::{
    GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE, GMEM_ZEROINIT,
};
use windows::Win32::System::Ole::CF_UNICODETEXT;
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::Win32::UI::WindowsAndMessaging::WM_CLIPBOARDUPDATE;

/// 外壳 wndproc 里 match 用的消息号
pub const UPDATED: u32 = WM_CLIPBOARDUPDATE;

/// 把剪贴板变更通知挂到窗口上
pub fn watch(hwnd: HWND) -> bool {
    unsafe { AddClipboardFormatListener(hwnd).is_ok() }
}

/// 窗口要没了，先把监听摘掉。不摘也不会崩，但系统会一直往一个已销毁的 HWND 上投递消息
pub fn unwatch(hwnd: HWND) {
    unsafe {
        let _ = RemoveClipboardFormatListener(hwnd);
    }
}

pub fn read_text() -> Option<String> {
    unsafe {
        IsClipboardFormatAvailable(CF_UNICODETEXT.0 as u32).ok()?;
        OpenClipboard(None).ok()?;
        let out = (|| {
            let handle = GetClipboardData(CF_UNICODETEXT.0 as u32).ok()?;
            let mem = HGLOBAL(handle.0);
            let ptr = GlobalLock(mem) as *const u16;
            if ptr.is_null() {
                return None;
            }
            let len = GlobalSize(mem) / std::mem::size_of::<u16>();
            let slice = std::slice::from_raw_parts(ptr, len);
            let end = slice.iter().position(|c| *c == 0).unwrap_or(slice.len());
            let text = String::from_utf16_lossy(&slice[..end]);
            let _ = GlobalUnlock(mem);
            Some(text)
        })();
        let _ = CloseClipboard();
        out
    }
}

/// 对应 pasteboard::write_text：失败返回 false，调用方按「没有可复制的内容」处理
pub fn write_text(text: &str) -> bool {
    unsafe {
        if OpenClipboard(None).is_err() {
            return false;
        }
        let ok = (|| -> Option<()> {
            let units: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
            let bytes = units.len() * std::mem::size_of::<u16>();
            let mem = GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, bytes).ok()?;
            let dst = GlobalLock(mem) as *mut u16;
            if dst.is_null() {
                let _ = GlobalFree(Some(mem));
                return None;
            }
            std::ptr::copy_nonoverlapping(units.as_ptr(), dst, units.len());
            let _ = GlobalUnlock(mem);
            // 到这一步才清空：EmptyClipboard 放前面的话，一旦分配失败，
            // 用户原来复制的东西已经被毁掉，而我们的东西又没写进去
            if EmptyClipboard().is_err() {
                // 清空失败也得先把手上这块还掉，`.ok()?` 会把它漏在进程里
                let _ = GlobalFree(Some(mem));
                return None;
            }
            // 成功后所有权交给系统，此时再 GlobalFree 会破坏剪贴板内容
            if SetClipboardData(CF_UNICODETEXT.0 as u32, Some(HANDLE(mem.0))).is_err() {
                let _ = GlobalFree(Some(mem));
                return None;
            }
            Some(())
        })();
        let _ = CloseClipboard();
        ok.is_some()
    }
}

/// 本地时间 HH:MM:SS，只用于状态行的 tooltip
pub fn local_time() -> String {
    unsafe {
        let t = GetLocalTime();
        format!("{:02}:{:02}:{:02}", t.wHour, t.wMinute, t.wSecond)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// 剪贴板是机器全局的，测试会覆盖用户当下复制的内容，所以前后各存/恢复一次
    struct Restore(Option<String>);
    impl Restore {
        fn new() -> Self {
            Self(read_text())
        }
    }
    impl Drop for Restore {
        fn drop(&mut self) {
            if let Some(text) = &self.0 {
                let _ = write_text(text);
            }
        }
    }

    fn exclusive<R>(f: impl FnOnce() -> R) -> R {
        // 剪贴板是进程外的机器全局资源，几个测试必须串行
        let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        f()
    }

    #[test]
    fn writes_and_reads_back_utf16_text() {
        exclusive(|| {
            let _restore = Restore::new();
            let text = "选区直通\r\n第二行 with English 和 emoji 🎯";
            assert!(write_text(text), "SetClipboardData 失败");
            assert_eq!(read_text().as_deref(), Some(text));
        });
    }

    /// 纯空白必须原样读回来 —— "算不算空"是 capture::from_clipboard 的判断，
    /// 剪贴板这一层不能替它决定（否则复制了一个空行，红点根本不会亮）
    #[test]
    fn whitespace_survives_the_round_trip() {
        exclusive(|| {
            let _restore = Restore::new();
            assert!(write_text(
                "   
	 "
            ));
            assert_eq!(
                read_text().as_deref(),
                Some(
                    "   
	 "
                )
            );
        });
    }
}
