//! Windows 外壳：无边框、不抢焦点、常驻顶层的 chip 窗口。
//! 对应 mac 侧 chip.rs 的 NSWindowStyleMask::Borderless | NonactivatingPanel。

use std::cell::RefCell;
use std::env;
use std::sync::atomic::{AtomicU32, Ordering};

use windows::core::w;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_SYSTEMBACKDROP_TYPE, DWMWA_WINDOW_CORNER_PREFERENCE,
    DWM_SYSTEMBACKDROP_TYPE, DWM_WINDOW_CORNER_PREFERENCE,
};
use windows::Win32::Graphics::Gdi::{
    ClientToScreen, GetMonitorInfoW, InvalidateRect, MonitorFromPoint, ValidateRect, MONITORINFO,
    MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{
    GetDpiForWindow, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetCursorPos, GetMessageW,
    GetWindowRect, KillTimer, LoadCursorW, PostQuitMessage, RegisterClassExW, SetTimer,
    SetWindowPos, ShowWindow, TranslateMessage, CS_HREDRAW, CS_VREDRAW, HTCAPTION, HTCLIENT,
    HWND_TOPMOST, IDC_ARROW, MSG, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SW_SHOW, WM_DESTROY,
    WM_LBUTTONUP, WM_MOUSEMOVE, WM_NCHITTEST, WM_PAINT, WM_SIZE, WM_TIMER, WNDCLASSEXW,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_POPUP,
};

use crate::clip;
use settings::AppSettings;

const WIDTH: i32 = 400;
const HEIGHT: i32 = 44;
const MARGIN_RIGHT: i32 = 16;
const MARGIN_TOP: i32 = 60;
const SMOKE_TIMER: usize = 0xA0;
const HEARTBEAT: usize = 0xA1;
const SAVETICK: usize = 0xA2;
const FLASH: usize = 0xA3;
/// 与 mac 侧 app.rs 的 ERROR_HOLD / FLASH_HOLD 同一口径
const ERROR_HOLD_MS: u32 = 3000;
const FLASH_HOLD_MS: u32 = 4000;
/// 唯一不算错误的提示：设置保存成功。别的 flash 一律按错误染成 --warn
const NOTICE: &str = "已保存";

static MSGS: AtomicU32 = AtomicU32::new(0);
thread_local! {
    static MOVED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static CLICKED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static COPIED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// UI 线程状态。COM/GDI 句柄都不是 Send，只能待在 thread_local 里
struct Ui {
    chip: crate::paint::Chip,
    layout: crate::paint::Layout,
    painted: bool,
    /// 上次落盘的位置（DIP）；只在真的变了时才写文件
    saved: Option<(f64, f64)>,
    /// 选区 + 会话上下文那一「包」，与 mac 共用 pack crate
    pack: pack::Pack,
    /// 面板/外壳都要用的那份设置；M4 加设置界面后改成每次读
    settings: AppSettings,
    /// pack 组装出来的待复制内容：直通是选区逐字节，会话解读是套模板的 Prompt
    payload: Option<pack::Payload>,
    /// 未读复制（红点携带的信息），取入或自己写回后清掉
    unread: Option<capture::ClipNote>,
    /// 上次看到的剪贴板内容，用来判断"这是别人复制的"还是"我们自己的写回"
    last_clip: String,
    /// 一两秒后自动消失的错误提示；None 表示状态行回到正常的选区文案
    error: Option<String>,
}

thread_local! {
    static UI: RefCell<Ui> = RefCell::new(Ui {
        chip: crate::paint::Chip::default(),
        layout: crate::paint::Layout::default(),
        painted: false,
        saved: None,
        pack: pack::Pack::default(),
        settings: AppSettings::default(),
        payload: None,
        unread: None,
        last_clip: String::new(),
        error: None,
    });
}

pub fn run() -> windows::core::Result<()> {
    // 必须先于建窗：布局按 DPI 换算，进程不感知 DPI 就量不准
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
    unsafe { RegisterClassExW(&wndclass) };

    let work = unsafe { work_area() };
    // 不加 WS_EX_NOREDIRECTIONBITMAP：那是给 D3D/合成器直呈准备的，GDI 画上去会因为没有
    // 重定向表面而完全不可见（实测：窗口在、圆角在，内容全是桌面）。M0 用它做过材质实验，已收。
    let hwnd = unsafe {
        CreateWindowExW(
            // TOOLWINDOW ≈ 不进任务栏 / ⌘Tab（mac 侧的 Accessory 策略）
            // NOACTIVATE ≈ focusable:false：点它不把键盘焦点从源应用抢走
            WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            class,
            w!("select-assist"),
            WS_POPUP,
            work.right - WIDTH - MARGIN_RIGHT,
            work.top + MARGIN_TOP,
            WIDTH,
            HEIGHT,
            None,
            None,
            Some(HINSTANCE(instance.0)),
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
        // ShowWindow 不保证产生 WM_PAINT，显式失效一次
        let _ = InvalidateRect(Some(hwnd), None, true);
    }

    if env::var("SA_WEBVIEW").is_ok() {
        let _ = unsafe {
            windows::Win32::System::Com::CoInitializeEx(
                None,
                windows::Win32::System::Com::COINIT_APARTMENTTHREADED,
            )
        };
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

    // 设置只在启动时读一次：M4 加设置界面后改成每次用时现读
    let settings = store().load();
    UI.with(|u| u.borrow_mut().settings = settings);
    if !clip::watch(hwnd) {
        println!("剪贴板监听注册失败，未读点不会亮");
    }
    // 开机前就已经复制过的内容也要标成未读（mac 侧用不可能的 changeCount 达到同一效果）
    on_clipboard_update(hwnd);
    unsafe { apply_saved_position(hwnd) };
    // 把当前落点当作基准存下来：只有跟它不同才是用户真的拖动过，避免每次开机白写一遍文件
    UI.with(|u| {
        let mut r = RECT::default();
        unsafe {
            let _ = GetWindowRect(hwnd, &mut r);
            let scale = GetDpiForWindow(hwnd) as f64 / 96.0;
            u.borrow_mut().saved = Some((r.left as f64 / scale, r.top as f64 / scale));
        }
    });
    // 位置落盘：WS_EX_NOACTIVATE 的窗口收不到 WM_EXITSIZEMOVE（实测），
    // 所以用 1 秒一次的比较式轮询，只在真的移动过才写
    let _ = unsafe { SetTimer(Some(hwnd), SAVETICK, 1000, None) };
    if let Some(ms) = env::var("SA_SMOKE_MS").ok().and_then(|v| v.parse().ok()) {
        let _ = unsafe { SetTimer(Some(hwnd), SMOKE_TIMER, ms, None) };
    }
    unsafe { report(hwnd) };

    let mut msg = MSG::default();
    while unsafe { GetMessageW(&mut msg, None, 0, 0).as_bool() } {
        let _ = unsafe { TranslateMessage(&msg) };
        unsafe { DispatchMessageW(&msg) };
    }
    Ok(())
}

/// SA_BACKDROP=acrylic|mica 控制，用来在原生窗口上复现「DWM 材质 + 44 高」会发生什么。
/// 圆角始终打开：卡片本身就是窗口，交给 DWM 裁比自绘抗锯齿省事。
unsafe fn apply_material(hwnd: HWND) {
    let corner = DWM_WINDOW_CORNER_PREFERENCE(2); // DWMWCP_ROUND
    let _ = DwmSetWindowAttribute(
        hwnd,
        DWMWA_WINDOW_CORNER_PREFERENCE,
        &corner as *const _ as *const _,
        std::mem::size_of::<DWM_WINDOW_CORNER_PREFERENCE>() as u32,
    );
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
}

/// 上次的位置（DIP 存在 settings.json 里，和 Electron 版同一份文件）。
/// 显示器拔掉之后靠 MonitorFromPoint 的"最近显示器"把窗口拉回屏内，别让它跑到无限远处
unsafe fn apply_saved_position(hwnd: HWND) {
    let Some((x, y)) = store().position() else {
        return;
    };
    let scale = GetDpiForWindow(hwnd) as f32 / 96.0;
    let mut pt = POINT {
        x: (x as f32 * scale) as i32,
        y: (y as f32 * scale) as i32,
    };
    let mut info = monitor_info();
    if !GetMonitorInfoW(MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST), &mut info).as_bool() {
        return;
    }
    let wa = info.rcWork;
    pt.x =
        pt.x.min(wa.right - (WIDTH as f32 * scale) as i32)
            .max(wa.left);
    pt.y =
        pt.y.min(wa.bottom - (HEIGHT as f32 * scale) as i32)
            .max(wa.top);
    let _ = SetWindowPos(hwnd, None, pt.x, pt.y, 0, 0, SWP_NOSIZE | SWP_NOACTIVATE);
}

/// SA_SETTINGS 指到别处时用它，避免冒烟测试写进用户真实的 settings.json
/// （dirs::config_dir() 走 SHGetKnownFolderPath，改 APPDATA 环境变量是没用的，实测过）
fn store() -> settings::Settings {
    match env::var("SA_SETTINGS") {
        Ok(p) => settings::Settings::at(std::path::PathBuf::from(p)),
        Err(_) => settings::Settings::shared(),
    }
}

/// 取入选区：读剪贴板 → 存进 pack → 会话解读模式再挂一次上下文。
/// 空白剪贴板不算取入，只提示。对应 mac 的 capture_selection，面板那几步 M4 补。
fn capture_selection(hwnd: HWND) {
    let text = clip::read_text().unwrap_or_default();
    let selection = match capture::from_clipboard(&text, &clip::local_time()) {
        Ok(selection) => selection,
        Err(reason) => return flash(hwnd, &reason),
    };
    UI.with(|u| {
        let mut ui = u.borrow_mut();
        ui.last_clip = text;
        ui.pack.set_selection(selection);
        ui.unread = None;
        ui.error = None;
        let settings = ui.settings.clone();
        if settings.with_context {
            // agent 先固定 auto：面板的会话下拉是 M4 的事，那时换成 panel 选中的 token
            ui.pack
                .attach(&settings, "auto", settings.context_turns, None, None);
        }
        ui.payload = ui.pack.payload(&settings);
    });
    let _ = unsafe { InvalidateRect(Some(hwnd), None, false) };
}

/// 复制：直通是选区逐字节，会话解读是组装后的 Prompt —— 两者都是 payload.prompt，
/// 分工在 pack 里，不在这里。mac 侧的文案原样沿用，两版不一致时一起改。
fn copy_payload(hwnd: HWND) {
    // 写回失败和没有 payload 是同一件事：现在没有可复制的东西（mac 侧同一个文案）
    let prompt = UI.with(|u| u.borrow().payload.as_ref().map(|p| p.prompt.clone()));
    let Some(prompt) = prompt else {
        flash(hwnd, "还没有取入选区");
        return;
    };
    if !clip::write_text(&prompt) {
        flash(hwnd, "还没有取入选区");
        return;
    }
    // 自己写回的东西不能反过来点亮未读点（Electron 在 pack:copy 里同步 lastClip）
    UI.with(|u| u.borrow_mut().last_clip = prompt);
    let _ = unsafe { InvalidateRect(Some(hwnd), None, false) };
}

/// 别人复制的新内容才点亮未读点；自己写回的不算（mac 侧同理，靠同步 lastClip 实现）
fn on_clipboard_update(hwnd: HWND) {
    let Some(text) = clip::read_text() else {
        return;
    };
    if text.trim().is_empty() {
        return;
    }
    let changed = UI.with(|u| {
        let mut ui = u.borrow_mut();
        if ui.last_clip == text {
            false
        } else {
            ui.last_clip = text.clone();
            ui.unread = Some(capture::note_from(&text));
            true
        }
    });
    if changed {
        let _ = unsafe { InvalidateRect(Some(hwnd), None, false) };
    }
}

fn flash(hwnd: HWND, message: &str) {
    let is_error = message != NOTICE;
    UI.with(|u| u.borrow_mut().error = is_error.then(|| message.to_string()));
    // 状态行只由 WM_PAINT 从 error / selection 推出来，这里只负责叫醒重画
    let _ = unsafe { InvalidateRect(Some(hwnd), None, false) };
    // 提示会自己收回，状态行回到选区文案（对应 mac 侧的 after(_:After::ClearFlash)）
    let hold = if is_error {
        ERROR_HOLD_MS
    } else {
        FLASH_HOLD_MS
    };
    let _ = unsafe { SetTimer(Some(hwnd), FLASH, hold, None) };
}

fn save_position_if_moved(hwnd: HWND) {
    unsafe {
        let mut r = RECT::default();
        let _ = GetWindowRect(hwnd, &mut r);
        let scale = GetDpiForWindow(hwnd) as f64 / 96.0;
        let pos = (r.left as f64 / scale, r.top as f64 / scale);
        let changed = UI.with(|u| {
            let mut ui = u.borrow_mut();
            if ui.saved == Some(pos) {
                false
            } else {
                ui.saved = Some(pos);
                true
            }
        });
        if !changed {
            return;
        }
        if let Err(e) = store().patch_position(pos.0, pos.1) {
            println!("存位置失败: {e}");
        }
    }
}

/// GetMonitorInfoW 要求先填 cbSize，两处都要，单独收一下
fn monitor_info() -> MONITORINFO {
    MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    }
}

/// 工作区（扣掉任务栏）。取光标所在的那块屏，和 mac 侧 workArea 语义一致
unsafe fn work_area() -> RECT {
    let mut cursor = POINT::default();
    GetCursorPos(&mut cursor).ok();
    let monitor = MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST);
    let mut info = monitor_info();
    if GetMonitorInfoW(monitor, &mut info).as_bool() {
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
        "app-win pid={} hwnd={:#x} rect={}x{}@{},{} dpi={}",
        std::process::id(),
        hwnd.0 as usize,
        r.right - r.left,
        r.bottom - r.top,
        r.left,
        r.top,
        GetDpiForWindow(hwnd)
    );
}

/// WM_NCHITTEST 的 lParam 是屏幕坐标，换成客户区坐标
unsafe fn client_point(hwnd: HWND, l: LPARAM) -> (i32, i32) {
    let mut origin = POINT { x: 0, y: 0 };
    let _ = ClientToScreen(hwnd, &mut origin);
    let sx = (l.0 as i32 & 0xFFFF) as i16 as i32;
    let sy = ((l.0 >> 16) as i32 & 0xFFFF) as i16 as i32;
    (sx - origin.x, sy - origin.y)
}

fn lparam_point(l: LPARAM) -> (i32, i32) {
    (
        (l.0 as u32 & 0xFFFF) as i16 as i32,
        ((l.0 as u32) >> 16 & 0xFFFF) as i16 as i32,
    )
}

fn hit(hwnd: HWND, pt: (i32, i32)) -> Option<&'static str> {
    UI.with(|u| {
        crate::paint::hit(&u.borrow().layout, pt, unsafe { GetDpiForWindow(hwnd) }
            as f32)
    })
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    MSGS.fetch_add(1, Ordering::Relaxed);
    match msg {
        WM_PAINT => {
            let layout = UI.with(|u| {
                let mut ui = u.borrow_mut();
                // 状态行只有一个来源：有错误报错误，否则报选区（同 mac 的 refresh）
                match ui.error.clone() {
                    Some(message) => {
                        ui.chip.status = message;
                        ui.chip.status_err = true;
                    }
                    None => {
                        ui.chip.status = ui.pack.status_line().0;
                        ui.chip.status_err = false;
                    }
                }
                ui.chip.badge = ui.unread.is_some();
                let l = crate::paint::paint(hwnd, &ui.chip);
                ui.layout = l;
                ui.painted = true;
                l
            });
            let _ = ValidateRect(Some(hwnd), None);
            let _ = layout;
            LRESULT(0)
        }
        clip::UPDATED => {
            on_clipboard_update(hwnd);
            LRESULT(0)
        }
        WM_SIZE => {
            let _ = InvalidateRect(Some(hwnd), None, false);
            LRESULT(0)
        }

        WM_MOUSEMOVE => {
            let over = matches!(hit(hwnd, lparam_point(l)), Some("button"));
            let changed = UI.with(|u| {
                let mut ui = u.borrow_mut();
                if ui.chip.hover != over {
                    ui.chip.hover = over;
                    true
                } else {
                    false
                }
            });
            if changed {
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
            LRESULT(0)
        }
        // 除按钮/把手外都是拖拽区（对应 CSS 的 -webkit-app-region: drag）
        WM_NCHITTEST => {
            let on_control = hit(hwnd, client_point(hwnd, l)).is_some();
            LRESULT(if on_control { HTCLIENT } else { HTCAPTION } as isize)
        }
        WM_LBUTTONUP => {
            match hit(hwnd, lparam_point(l)) {
                Some("button") => capture_selection(hwnd),
                Some("dot") => flash(hwnd, "展开面板（M4 接）"),
                _ => {}
            }
            let _ = InvalidateRect(Some(hwnd), None, false);
            LRESULT(0)
        }
        WM_TIMER => {
            match w.0 {
                SMOKE_TIMER => {
                    let painted = UI.with(|u| u.borrow().painted);
                    println!(
                        "smoke 到期，首帧已画={painted}，收到消息 {} 条",
                        MSGS.load(Ordering::Relaxed)
                    );
                    let _ = DestroyWindow(hwnd);
                }
                HEARTBEAT => crate::webview::tick(hwnd),
                FLASH => {
                    UI.with(|u| u.borrow_mut().error = None);
                    let _ = KillTimer(Some(hwnd), FLASH);
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
                SAVETICK => {
                    save_position_if_moved(hwnd);
                    // SA_MOVE / SA_CAPTURE：自己挪一下 / 自己取入一次，用来验证
                    // 「移动 -> 落盘 -> 重启恢复」和「复制 -> 未读点 -> 取入 -> 状态行」这两条链，
                    // 不依赖合成鼠标事件（那玩意儿在这台机器上不可靠）
                    if env::var("SA_MOVE").is_ok() && !MOVED.with(|m| m.get()) {
                        MOVED.with(|m| m.set(true));
                        let _ =
                            SetWindowPos(hwnd, None, 120, 120, 0, 0, SWP_NOSIZE | SWP_NOACTIVATE);
                    }
                    // SA_COPY 隐含 SA_CAPTURE，但错开一轮触发：这样截图能把「已取入」和
                    // 「写回后未读点仍然不亮」分成两帧看到
                    let had_capture = CLICKED.with(|c| c.get());
                    let wants_copy = env::var("SA_COPY").is_ok();
                    if (env::var("SA_CAPTURE").is_ok() || wants_copy) && !had_capture {
                        CLICKED.with(|c| c.set(true));
                        capture_selection(hwnd);
                    }
                    if wants_copy && had_capture && !COPIED.with(|c| c.get()) {
                        COPIED.with(|c| c.set(true));
                        copy_payload(hwnd);
                    }
                }
                _ => {}
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
