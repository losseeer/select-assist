//! M0 窗口骨架：无边框、不抢焦点、常驻顶层、按 DPI 换算物理尺寸。
//! 对应 mac 侧 chip.rs 的 NSWindowStyleMask::Borderless | NonactivatingPanel。

use std::env;

use windows::core::w;
use windows::Win32::Foundation::{COLORREF, HWND, HINSTANCE, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_SYSTEMBACKDROP_TYPE, DWMWA_WINDOW_CORNER_PREFERENCE,
    DWM_SYSTEMBACKDROP_TYPE, DWM_WINDOW_CORNER_PREFERENCE,
};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateSolidBrush, DeleteObject, EndPaint, FillRect, GetMonitorInfoW, HBRUSH,
    MONITORINFO, MONITOR_DEFAULTTONEAREST, MonitorFromPoint, PAINTSTRUCT, HGDIOBJ,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{
    GetDpiForWindow, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetCursorPos, GetMessageW,
    GetWindowRect, LoadCursorW, PostQuitMessage, RegisterClassExW, SetTimer, SetWindowPos,
    ShowWindow, TranslateMessage, CS_HREDRAW, CS_VREDRAW, HTCAPTION, HWND_TOPMOST, IDC_ARROW,
    MSG, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SW_SHOW,
    WM_DESTROY, WM_NCHITTEST, WM_PAINT, WM_TIMER, WNDCLASSEXW, WS_EX_NOACTIVATE,
    WS_EX_LAYERED, WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW, WS_POPUP,
};


const WIDTH: i32 = 400;
const HEIGHT: i32 = 44;
const MARGIN_RIGHT: i32 = 16;
const MARGIN_TOP: i32 = 60;
const SMOKE_TIMER: usize = 0xA0;
const HEARTBEAT: usize = 0xA1;

pub fn run() -> windows::core::Result<()> {
    // 必须先于建窗：DWM 那个最小高度按物理像素算，进程不感知 DPI 就量不准
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    };

    let instance = unsafe { GetModuleHandleW(None)? };
    let class = w!("SelectAssistNativeChip");
    let wndclass = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(wndproc),
        hInstance: HINSTANCE(instance.0),
        hCursor: unsafe { LoadCursorW(None, IDC_ARROW)? },
        lpszClassName: class,
        ..Default::default()
    };
    let _atom = unsafe { RegisterClassExW(&wndclass) };
    let _ = _atom;

    // SA_LAYERED=1 复现 Electron 的 transparent:true（Chromium 走 layered + 材质的组合）
    let ex_style = if env::var("SA_LAYERED").is_ok() {
        WS_EX_LAYERED
    } else if env::var("SA_OPAQUE").is_ok() {
        WS_EX_TOOLWINDOW // 占位：什么都不加
    } else {
        WS_EX_NOREDIRECTIONBITMAP
    };
    let work = unsafe { work_area() };
    let hwnd = unsafe {
        CreateWindowExW(
            // TOOLWINDOW ≈ 不进任务栏 / ⌘Tab（mac 侧的 Accessory 策略）
            // NOACTIVATE ≈ focusable:false：点它不把键盘焦点从源应用抢走
            ex_style | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            class,
            w!("select-assist"),
            WS_POPUP,
            work.right - WIDTH - MARGIN_RIGHT,
            work.top + MARGIN_TOP,
            WIDTH,
            HEIGHT,
            None,
            None,
            Some(instance.into()),
            None,
        )?
    };

    unsafe { apply_material(hwnd) };
    unsafe {
        // 常驻顶层 ≈ mac 的 screen-saver window level
        SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE,
        )?;
        let _ = ShowWindow(hwnd, SW_SHOW);
    }
    if env::var("SA_WEBVIEW").is_ok() {
        let _ = unsafe {
            windows::Win32::System::Com::CoInitializeEx(
                None,
                windows::Win32::System::Com::COINIT_APARTMENTTHREADED,
            )
        };
        // 页面由外部给（不写死机器路径）：SA_WEBVIEW_URL=file:///.../static/index.html#chip
        match env::var("SA_WEBVIEW_URL").ok() {
            Some(url) => {
                // 用户数据目录单独放，别在 target/ 里留 WebView2 的垃圾
                let data = std::env::temp_dir().join("sa-webview-experiment");
                std::fs::create_dir_all(&data).ok();
                crate::webview::start(hwnd, &data.to_string_lossy(), &url);
                let _ = unsafe { SetTimer(Some(hwnd), HEARTBEAT, 50, None) };
            }
            None => println!("SA_WEBVIEW=1 但没给 SA_WEBVIEW_URL，跳过"),
        }
    }
    if let Some(ms) = env::var("SA_SMOKE_MS").ok().and_then(|v| v.parse().ok()) {
        let _ = unsafe { SetTimer(Some(hwnd), SMOKE_TIMER, ms, None) };
        // SetTimer 返回 0 才算失败，M0 不关心
    }
    unsafe { report(hwnd) };

    let mut msg = MSG::default();
    while unsafe { GetMessageW(&mut msg, None, 0, 0).as_bool() } {
        unsafe {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    Ok(())
}

/// SA_BACKDROP=acrylic|mica 控制，用来在原生窗口上复现「DWM 材质 + 44 高」会发生什么
unsafe fn apply_material(hwnd: HWND) {
    let backdrop = match env::var("SA_BACKDROP").unwrap_or_default().as_str() {
        "acrylic" => DWM_SYSTEMBACKDROP_TYPE(3),
        "mica" => DWM_SYSTEMBACKDROP_TYPE(2),
        _ => return,
    };
    let _ = DwmSetWindowAttribute(
        hwnd,
        DWMWA_SYSTEMBACKDROP_TYPE,
        &backdrop as *const _ as *const _,
        std::mem::size_of::<DWM_SYSTEMBACKDROP_TYPE>() as u32,
    );
    let corner = DWM_WINDOW_CORNER_PREFERENCE(2); // DO_ROUND
    let _ = DwmSetWindowAttribute(
        hwnd,
        DWMWA_WINDOW_CORNER_PREFERENCE,
        &corner as *const _ as *const _,
        std::mem::size_of::<DWM_WINDOW_CORNER_PREFERENCE>() as u32,
    );
}

/// 工作区（扣掉任务栏）。取光标所在的那块屏，和 mac 侧 workArea 语义一致
unsafe fn work_area() -> RECT {
    let mut cursor = POINT::default();
    GetCursorPos(&mut cursor).ok();
    let mut info = MONITORINFO::default();
    info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
    if GetMonitorInfoW(MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST), &mut info).as_bool() {
        return info.rcWork;
    }
    RECT {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1080,
    }
}

unsafe fn report(hwnd: HWND) {
    let mut r = RECT::default();
    let _ = GetWindowRect(hwnd, &mut r);
    println!(
        "app-win pid={} rect={}x{}@{},{} dpi={}",
        std::process::id(),
        r.right - r.left,
        r.bottom - r.top,
        r.left,
        r.top,
        GetDpiForWindow(hwnd)
    );
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);
            // #161618：--bg 叠在无材质窗口上的实测色，M0 只用来证明画上了
            let brush = CreateSolidBrush(COLORREF(0x0018_1616));
            FillRect(hdc, &ps.rcPaint, HBRUSH(brush.0));
            let _ = DeleteObject(HGDIOBJ(brush.0));
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        // 整条 chip 都是拖拽区（对应 CSS 的 -webkit-app-region: drag）
        WM_NCHITTEST => LRESULT(HTCAPTION as isize),
        WM_TIMER => {
            if w.0 == SMOKE_TIMER {
                let _ = DestroyWindow(hwnd);
            } else if w.0 == HEARTBEAT {
                crate::webview::tick(hwnd);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, w, l),
    }
}
