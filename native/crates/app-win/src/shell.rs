//! Windows 外壳：无边框、不抢焦点、常驻顶层的 chip 窗口。
//! 对应 mac 侧 chip.rs 的 NSWindowStyleMask::Borderless | NonactivatingPanel。

use std::cell::RefCell;
use std::env;
use std::sync::atomic::{AtomicU32, Ordering};

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWM_WINDOW_CORNER_PREFERENCE,
};
use windows::Win32::Graphics::Gdi::{
    ClientToScreen, GetMonitorInfoW, InvalidateRect, MonitorFromPoint, MonitorFromRect,
    ValidateRect, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{
    GetDpiForWindow, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CheckMenuItem, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu,
    DestroyWindow, DispatchMessageW, GetClientRect, GetCursorPos, GetMessageW, GetWindowRect,
    KillTimer, LoadCursorW, PostMessageW, PostQuitMessage, RegisterClassExW, SetForegroundWindow,
    SetTimer, SetWindowPos, ShowWindow, TrackPopupMenu, TranslateMessage, CS_HREDRAW, CS_VREDRAW,
    EN_CHANGE, HTCAPTION, HTCLIENT, HWND_TOPMOST, IDC_ARROW, MF_BYCOMMAND, MF_CHECKED, MF_STRING,
    MSG, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SW_HIDE, SW_SHOW, SW_SHOWNORMAL,
    TPM_BOTTOMALIGN, TPM_LEFTBUTTON, TPM_RETURNCMD, WM_COMMAND, WM_CTLCOLOREDIT, WM_DESTROY,
    WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NCHITTEST, WM_NULL, WM_PAINT, WM_SIZE, WM_TIMER,
    WNDCLASSEXW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_POPUP,
};

use crate::clip;
use crate::draw::{self, RectF};
use crate::edits;
use crate::panel;
use crate::theme;
use settings::AppSettings;

/// Electron defaultChipPos()：主屏 workArea 右上角内缩 16 / 60。
/// 宽高本身在 crate::theme 里，和绘制共用同一个数
const MARGIN_RIGHT: i32 = 16;
const MARGIN_TOP: i32 = 60;
const WIDTH: i32 = theme::WIDTH as i32;
const HEIGHT: i32 = theme::CHIP_H as i32;
const SMOKE_TIMER: usize = 0xA0;
const SAVETICK: usize = 0xA2;
const FLASH: usize = 0xA3;
const COPYHOLD: usize = 0xA4;
/// 「已复制 ✓」的停留时间，与 mac 的 COPIED_HOLD 一致
const COPIED_HOLD_MS: u32 = 1200;
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
    static PANELD: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// UI 线程状态。COM/GDI 句柄都不是 Send，只能待在 thread_local 里
struct Ui {
    /// chip 自己的 HWND。面板的动作要回调 chip（取入 / 折叠 / 退出），
    /// 而 wndproc 的 hwnd 参数在定时器里没有，所以存一份
    hwnd: HWND,
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
    /// 非错误的即时提示（「已保存」），显示在面板的上下文状态位上，同样会自己收回
    notice: Option<String>,
    // ---- 面板（M4）。HWND 为 0 表示没开 ----
    panel: HWND,
    panel_layout: panel::Layout,
    /// 面板上那几个下拉当前选中的值：agent 是外壳的视图状态，不在 settings 里
    agent: String,
    browsing: bool,
    browser_rows: Vec<panel::BrowserRow>,
    /// 与 browser_rows 同序的会话引用，点第 i 行要知道挂的是哪个文件
    browser_refs: Vec<ctxpack::adapters::SessionRef>,
    browser_sel: Option<usize>,
    /// 复制成功后「已复制 ✓」停留期间为 true
    copied: bool,
    panel_hover: Option<&'static str>,
    panel_row: Option<usize>,
    /// 正文滚动量与窗口当前高度（DIP）。窗口被工作区夹住时靠这两个数算滚动边界
    scroll: f32,
    panel_window_h: f32,
    /// 设置组是否展开。展开时才建那三个 EDIT —— 收着的时候它们是零个窗口
    settings_open: bool,
    /// 设置组里有没保存的改动
    dirty: bool,
    /// 正在编辑第几条指令的模板。跟 active_prompt 分开：草稿没保存前，
    /// 输出组那个下拉显示的仍然是已生效的那条
    draft_prompt: usize,
    /// 站点框的草稿，按模式分两槽 [会话解读, 直通]（mac 的 sites_draft 同构）。
    /// 只有一槽的话，切模式时框里还是上一个模式的文字，保存就写进另一个列表 ——
    /// 实测过：在会话解读里改的站点，切到直通保存后进了 directSites，chatSites 原样没动，
    /// 而 Google/BingCN 两条被覆盖掉了。这份文件 Electron 也在读，等于毁掉用户的配置。
    sites_draft: [String; 2],
    /// 指令列表的草稿（mac 的 state.prompts 同构，跟 app.prompts 分开）。
    /// 不分家的时候"新建"出来的空指令会被下一个下拉操作 persist_settings 顺手写进
    /// settings.json —— 用户从没点过保存，Electron 那边却多了一条空指令；
    /// "删除"同理，没保存就先少一条。保存那一刻才把草稿过滤掉空条目后写回。
    draft_prompts: Vec<settings::PromptTemplate>,
}

thread_local! {
    static UI: RefCell<Ui> = RefCell::new(Ui {
        hwnd: HWND::default(),
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
        notice: None,
        panel: HWND::default(),
        panel_layout: panel::Layout::default(),
        agent: "auto".into(),
        browsing: false,
        browser_rows: Vec::new(),
        browser_refs: Vec::new(),
        browser_sel: None,
        copied: false,
        panel_hover: None,
        panel_row: None,
        scroll: 0.0,
        panel_window_h: 240.0,
        settings_open: false,
        dirty: false,
        draft_prompt: 0,
        sites_draft: [String::new(), String::new()],
        draft_prompts: Vec::new(),
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
    // 面板要能吃键盘（M4d 的编辑框），所以是另一个类：不带 NOACTIVATE，走自己的 wndproc
    unsafe {
        RegisterClassExW(&WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(panel_wndproc),
            hInstance: HINSTANCE(instance.0),
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            lpszClassName: w!("SelectAssistNativePanel"),
            ..Default::default()
        })
    };

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

    UI.with(|u| u.borrow_mut().hwnd = hwnd);
    // 设置只在启动时读一次：M4 加设置界面后改成每次用时现读
    let settings = store().load();
    UI.with(|u| {
        let mut ui = u.borrow_mut();
        // 草稿从已保存的那份起步（mac 的 state.prompts = app.prompts 同一步）
        ui.draft_prompts = settings.prompts.clone();
        ui.draft_prompt = settings
            .active_prompt
            .min(settings.prompts.len().saturating_sub(1));
        ui.settings = settings;
    });
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
    unsafe { probe_start(hwnd) };
    unsafe { report(hwnd) };

    let mut msg = MSG::default();
    while unsafe { GetMessageW(&mut msg, None, 0, 0).as_bool() } {
        let _ = unsafe { TranslateMessage(&msg) };
        unsafe { DispatchMessageW(&msg) };
    }
    Ok(())
}

/* ---------- 冒烟探针：只在 debug 构建里存在 ----------
"移动 -> 落盘 -> 重启恢复""复制 -> 未读点 -> 取入 -> 状态行"这几条链改成让程序
自己触发，这样锁屏、没有交互桌面的时候也能跑。真实输入那条路本身是通的：早先记的
"本机合成鼠标不可靠（点不到/落点飘）"其实是 scripts/input.ps1 漏了
MOUSEEVENTF_ABSOLUTE，落点全被钉在屏幕角上，探针因此一度看起来是唯一选项。
release 产物里这些代码整个不参与编译：能拿环境变量驱动真实状态机的 exe 不该发出去。 */

#[cfg(debug_assertions)]
unsafe fn probe_start(hwnd: HWND) {
    if let Some(ms) = env::var("SA_SMOKE_MS").ok().and_then(|v| v.parse().ok()) {
        let _ = SetTimer(Some(hwnd), SMOKE_TIMER, ms, None);
    }
}

#[cfg(debug_assertions)]
unsafe fn probe(hwnd: HWND) {
    if env::var("SA_MOVE").is_ok() && !MOVED.with(|m| m.get()) {
        MOVED.with(|m| m.set(true));
        let _ = SetWindowPos(hwnd, None, 120, 120, 0, 0, SWP_NOSIZE | SWP_NOACTIVATE);
    }
    // SA_COPY 隐含 SA_CAPTURE，但错开一轮：截图才能把「已取入」和
    // 「写回后未读点仍然不亮」分成两帧看到
    let had_capture = CLICKED.with(|c| c.get());
    let wants_copy = env::var("SA_COPY").is_ok();
    if (env::var("SA_CAPTURE").is_ok() || wants_copy) && !had_capture {
        CLICKED.with(|c| c.set(true));
        capture_selection(hwnd);
    }
    if env::var("SA_PANEL").is_ok() && !PANELD.with(|c| c.get()) {
        PANELD.with(|c| c.set(true));
        open_panel(hwnd);
    }
    if wants_copy && had_capture && !COPIED.with(|c| c.get()) {
        COPIED.with(|c| c.set(true));
        copy_payload(hwnd);
    }
}

#[cfg(not(debug_assertions))]
unsafe fn probe_start(_hwnd: HWND) {}

#[cfg(not(debug_assertions))]
unsafe fn probe(_hwnd: HWND) {}

/// 圆角交给 DWM：卡片本身就是窗口，让系统裁比自绘抗锯齿省事，也不会出现
/// "圆角外一圈方角"那种只有截屏才看得出的错。
/// 材质（acrylic/mica）刻意不挂：M0 量过，原生窗口挂不挂材质都是精确 400x44，
/// 那个 64 物理像素下限是 Chromium 透明窗口自己的事，我们不需要为它让路。
unsafe fn apply_material(hwnd: HWND) {
    let corner = DWM_WINDOW_CORNER_PREFERENCE(2); // DWMWCP_ROUND
    let _ = DwmSetWindowAttribute(
        hwnd,
        DWMWA_WINDOW_CORNER_PREFERENCE,
        &corner as *const _ as *const _,
        std::mem::size_of::<DWM_WINDOW_CORNER_PREFERENCE>() as u32,
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
            // 用面板上选中的 agent，不再写死 "auto"：写死的时候下拉选了 codex、
            // 点「取入选区」却挂上一条 Qoder 会话，状态行跟下拉互相矛盾。
            // 新选区也不该沿用上一次在浏览器里挑的那条会话（mac 在取入时清 browsing
            // 并 attach(None)），所以这里同样清掉选中项、按 agent 重新找
            let agent = ui.agent.clone();
            ui.browser_sel = None;
            ui.pack
                .attach(&settings, &agent, settings.context_turns, None, None);
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
    UI.with(|u| {
        let mut ui = u.borrow_mut();
        ui.last_clip = prompt;
        ui.copied = true;
    });
    // 「已复制 ✓」停一下再收回（mac 的 COPIED_HOLD）。定时器挂在面板上：
    // 只有面板显示这个文案，chip 上没有复制按钮
    let panel = UI.with(|u| u.borrow().panel);
    if !panel.0.is_null() {
        let _ = unsafe { SetTimer(Some(panel), COPYHOLD, COPIED_HOLD_MS, None) };
    }
    let _ = unsafe { InvalidateRect(Some(hwnd), None, false) };
}

/// 别人复制的新内容才点亮未读点；自己写回的不算（mac 侧同理，靠同步 lastClip 实现）
fn on_clipboard_update(hwnd: HWND) {
    let Some(text) = clip::read_text() else {
        return;
    };
    // 判据是"空"而不是"全空白"：空白复制照样点亮红点，取入时才由 capture 说剪贴板为空。
    // 与 mac 的 poll 同一口径，两版对同一次复制的反应必须一样。
    if text.is_empty() {
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
    UI.with(|u| {
        let mut ui = u.borrow_mut();
        ui.error = is_error.then(|| message.to_string());
        // 「已保存」这类提示不是错误，不进状态行，走面板的上下文状态位（同 mac 的 set_ctx_status）
        if !is_error {
            ui.notice = Some(message.to_string());
        }
    });
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

/// 窗口自己那块屏的工作区。不能用 work_area()：那个按**光标**所在显示器取，
/// 双屏下光标在哪块屏，面板就会被夹到一块跟它无关的屏的尺寸上
unsafe fn work_area_of(hwnd: HWND) -> RECT {
    let mut r = RECT::default();
    let _ = GetWindowRect(hwnd, &mut r);
    let mut info = monitor_info();
    if GetMonitorInfoW(MonitorFromRect(&r, MONITOR_DEFAULTTONEAREST), &mut info).as_bool() {
        return info.rcWork;
    }
    r
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

/* ---------- 面板（M4） ---------- */

/// 从 Ui 状态投影出面板要画的东西。panel.rs 不认识 pack / settings，翻译只做这一次。
fn panel_view(ui: &Ui) -> panel::PanelView {
    let (status, status_err) = match ui.error.clone() {
        Some(message) => (message, true),
        None => (ui.pack.status_line().0, false),
    };
    let (session_line, _, session_err) = ui.pack.session_line(ui.settings.with_context);
    let sites = if ui.settings.with_context {
        &ui.settings.chat_sites
    } else {
        &ui.settings.direct_sites
    };
    // 文案由 pack 决定：直通说「选区原文」，会话解读才说「组装后」，这边不重写一遍
    let pack_meta = ui.payload.as_ref().map_or_else(String::new, |p| p.meta().0);
    let view = panel::PanelView {
        status,
        status_err,
        read_mode: ui.settings.with_context,
        agent: ui.agent.clone(),
        turns: ui.settings.context_turns,
        session_line,
        session_err,
        ctx_status: ui.notice.clone().unwrap_or_default(),
        ctx_err: false,
        browsing: ui.browsing,
        browser_rows: ui.browser_rows.clone(),
        browser_sel: ui.browser_sel,
        prompt_name: settings::active_prompt(&ui.settings)
            .map(|p| p.name.clone())
            .unwrap_or_else(|| "没有指令".into()),
        sites: sites.iter().map(|s| s.name.clone()).collect(),
        pack_meta,
        copied: ui.copied,
        hover: ui.panel_hover,
        hover_row: ui.panel_row,
        scroll: ui.scroll,
        content: ui.panel_layout.height,
        window: ui.panel_window_h,
        settings_open: ui.settings_open,
        dirty: ui.dirty,
        redact: ui.settings.redact_paths,
        // 这个下标标的是"框里正在编辑哪一条"，所以必须跟 draft_prompt 走：
        // 写 active_prompt 的话，上面那个下拉一切换，设置组就改成"指令 3 / 共 3 条"，
        // 而框里仍然是指令 1 的模板，保存进的是指令 1 —— 标签在骗人
        prompt_index: ui.draft_prompt,
        prompt_count: ui.draft_prompts.len(),
        sites_label: if ui.settings.with_context {
            "会话解读目标站".into()
        } else {
            "直通目标站".into()
        },
    };
    view
}

/// 展开：面板顶到 chip 同一个左上角，chip 让位（mac 是 orderOut + orderFront）
unsafe fn open_panel(chip: HWND) {
    if UI.with(|u| !u.borrow().panel.0.is_null()) {
        return;
    }
    let mut r = RECT::default();
    let _ = GetWindowRect(chip, &mut r);
    let s = draw::scale(chip);
    let panel = CreateWindowExW(
        // 只有 TOOLWINDOW：面板要能吃键盘，所以不带 NOACTIVATE（M4d 的编辑框靠它）
        WS_EX_TOOLWINDOW,
        w!("SelectAssistNativePanel"),
        w!("select-assist"),
        WS_POPUP,
        r.left,
        r.top,
        (theme::WIDTH * s) as i32,
        (240.0 * s) as i32, // 占位高度，第一帧 paint 之后按内容收敛
        None,
        None,
        None,
        None,
    )
    .unwrap_or_default();
    if panel.0.is_null() {
        return;
    }
    apply_material(panel);
    // 面板也要常驻顶层：chip 是，Electron 的两个窗口都是 alwaysOnTop，mac 的
    // views::panel() 统一设了 NSFloatingWindowLevel。少这一步，用户一点别的窗口，
    // 展开到一半的面板就被盖到后面去了 —— 而它是唯一能改设置、能退出程序的窗口。
    // SWP_NOACTIVATE：置顶不抢焦点，抢焦点交给 M4d 的编辑框自己决定。
    let _ = SetWindowPos(
        panel,
        Some(HWND_TOPMOST),
        0,
        0,
        0,
        0,
        SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE,
    );
    let _ = ShowWindow(panel, SW_SHOW);
    let _ = InvalidateRect(Some(panel), None, true);
    UI.with(|u| u.borrow_mut().panel = panel);
    let _ = ShowWindow(chip, SW_HIDE);
    // 设置组还开着的话，编辑框要跟着新窗口重建：它们是子窗口，随上一个面板一起销毁了
    if UI.with(|u| u.borrow().settings_open) {
        ensure_edits(panel);
    }
}

unsafe fn close_panel() {
    let panel = UI.with(|u| u.borrow().panel);
    if panel.0.is_null() {
        return;
    }
    let _ = DestroyWindow(panel);
    UI.with(|u| {
        let mut ui = u.borrow_mut();
        ui.panel = HWND::default();
        ui.panel_layout = panel::Layout::default();
        // 「已复制 ✓」的收回定时器挂在面板上，窗口一没就没人收，重开就是永久的高亮
        ui.copied = false;
        ui.panel_hover = None;
        ui.panel_row = None;
    });
}

unsafe fn quit_app(chip: HWND) {
    close_panel();
    let _ = DestroyWindow(chip);
}

/// 面板上的一次点击。id 来自 panel.rs 的命中表，那边每加一个这里就得加一条。
unsafe fn panel_action(chip: HWND, id: &'static str, row: Option<usize>) {
    match id {
        "collapse" => {
            close_panel();
            let _ = ShowWindow(chip, SW_SHOW);
            let _ = InvalidateRect(Some(chip), None, false);
        }
        "quit" => quit_app(chip),
        "capture" => capture_selection(chip),
        "mode-off" => set_mode(chip, !UI.with(|u| u.borrow().settings.with_context)),
        "browse" => browse_sessions(),
        "refresh" => refresh_context(),
        "browser" => pick_session(row),
        "copy" => copy_payload(chip),
        "agent" | "turns" | "prompt" => pick_menu(id),
        "site" => open_site(chip, row),
        "settings" => toggle_settings(chip),
        "prompt-pick" => pick_menu("prompt-pick"),
        "prompt-new" => new_prompt(chip),
        "prompt-del" => del_prompt(chip),
        "redact" => toggle_redact(chip),
        "save" => save_settings(chip),
        _ => {}
    }
}

/// 换模式。语义照 mac 的 set_mode：离开会话解读必须先把上下文清掉，
/// 否则"直通"复制出去的仍然是组装后的 Prompt —— pack 里有这条测试，壳里也得这么做。
fn set_mode(chip: HWND, read: bool) {
    if !UI.with(|u| apply_mode(&mut u.borrow_mut(), read)) {
        return;
    }
    // 站点框是跟着模式换的：切之前把当前内容收回本模式的草稿槽，
    // 不然新模式的框里还留着旧模式的文字，保存就串到另一个列表上
    if UI.with(|u| u.borrow().settings_open) {
        // apply_mode 已经把 with_context 翻过去了，所以"离开的那个模式"就是 !read，
        // 它的槽位下标是 usize::from(!(!read)) == usize::from(read)。
        // 写成 usize::from(!read) 会把会话解读的文字存进直通的槽里 —— 第一版就这么错了，
        // 是复现测试把它抓出来的：切到直通后框里仍然是会话解读那两条。
        let outgoing = usize::from(read);
        UI.with(|u| u.borrow_mut().sites_draft[outgoing] = edits::get(edits::SITES));
        unsafe { seed_edits() };
    }
    let settings = UI.with(|u| u.borrow().settings.clone());
    if read {
        attach_current(&settings);
    } else {
        recompute_payload(&settings);
    }
    // 模式是两版共用的设置，切完就落盘：不写回去，重启就回到旧模式，Electron 那边也看不到
    if let Err(e) = store().save(&settings) {
        println!("写模式失败: {e}");
    }
    repaint(chip);
}

/// 换模式的纯状态部分：返回 false 表示模式没变，调用方什么都不该做。
/// 离开会话解读一定要把上下文清掉 —— 不清的话"直通"复制出去的仍然是组装后的 Prompt，
/// pack 里有这条测试，壳里也必须这么做，所以单独拆出来给测试盯住。
fn apply_mode(ui: &mut Ui, read: bool) -> bool {
    if ui.settings.with_context == read {
        return false;
    }
    ui.settings.with_context = read;
    if !read {
        ui.browsing = false;
        ui.browser_sel = None;
        ui.pack.clear_context();
    }
    true
}

/// 用当前 agent、以及会话浏览器里挑中的那一条，去挂上下文并重算 payload
fn attach_current(settings: &settings::AppSettings) {
    let agent = UI.with(|u| u.borrow().agent.clone());
    let picked = UI.with(|u| {
        let ui = u.borrow();
        ui.browser_sel.and_then(|i| ui.browser_refs.get(i)).cloned()
    });
    UI.with(|u| {
        let mut ui = u.borrow_mut();
        ui.pack.attach(
            settings,
            &agent,
            settings.context_turns,
            picked.as_ref().map(|r| r.file_path.as_str()),
            picked.as_ref().and_then(|r| r.session_id.as_deref()),
        );
        ui.payload = ui.pack.payload(settings);
    });
}

fn recompute_payload(settings: &settings::AppSettings) {
    UI.with(|u| {
        let mut ui = u.borrow_mut();
        ui.payload = ui.pack.payload(settings);
    });
}

fn repaint(chip: HWND) {
    unsafe {
        let panel = UI.with(|u| u.borrow().panel);
        if !panel.0.is_null() {
            let _ = InvalidateRect(Some(panel), None, false);
        }
        let _ = InvalidateRect(Some(chip), None, false);
    }
}

/// 刷新 = 按当前 agent / 已挑的会话重新挂一次上下文（mac 的 attach(None)）。
/// 它刻意不重新发现：重新发现会把用户挑中的那一行冲掉。
fn refresh_context() {
    let settings = UI.with(|u| u.borrow().settings.clone());
    if settings.with_context {
        attach_current(&settings);
    } else {
        recompute_payload(&settings);
    }
}

/// 浏览会话：pack::browse 拿一批，列表按 mac 的口径显示「标题 · agent · 时间」
fn browse_sessions() {
    let refs = UI.with(|u| {
        let settings = u.borrow().settings.clone();
        pack::browse(&settings, pack::BROWSE_LIMIT)
    });
    let rows = refs
        .iter()
        .map(|r| {
            let (head, title, time) = pack::row_text(r);
            panel::BrowserRow {
                head,
                title,
                time,
                preview: r.preview.clone().unwrap_or_default(),
            }
        })
        .collect::<Vec<_>>();
    UI.with(|u| {
        let mut ui = u.borrow_mut();
        ui.browsing = true;
        ui.browser_sel = if rows.is_empty() { None } else { Some(0) };
        ui.browser_rows = rows;
        ui.browser_refs = refs;
    });
}

fn pick_session(row: Option<usize>) {
    let picked = UI.with(|u| {
        let ui = u.borrow();
        row.and_then(|i| ui.browser_refs.get(i)).cloned()
    });
    let Some(reference) = picked else {
        return;
    };
    UI.with(|u| {
        let mut ui = u.borrow_mut();
        ui.browser_sel = row;
        let settings = ui.settings.clone();
        let turns = settings.context_turns;
        ui.pack.attach(
            &settings,
            &reference.agent,
            turns,
            Some(&reference.file_path),
            reference.session_id.as_deref(),
        );
        ui.payload = ui.pack.payload(&settings);
    });
}

/* ---------- 下拉与站点 ---------- */

/// 候选值与 mac 的 agent_pick / turns_pick 同一张表，别在这儿加第 6 个 agent
const AGENTS: [&str; 6] = [
    "auto",
    "claude-code",
    "codex",
    "workbuddy",
    "qoder",
    "project",
];
const TURNS: [(&str, usize); 5] = [
    ("4 轮", 4),
    ("8 轮", 8),
    ("16 轮", 16),
    ("30 轮", 30),
    ("全部", 0),
];

/// 某个下拉的候选文字 + 当前选中的下标
fn menu_for(ui: &Ui, id: &'static str) -> Option<(Vec<String>, usize)> {
    match id {
        "agent" => {
            let current = AGENTS.iter().position(|a| *a == ui.agent).unwrap_or(0);
            Some((AGENTS.iter().map(|a| a.to_string()).collect(), current))
        }
        "turns" => {
            let current = TURNS
                .iter()
                .position(|(_, t)| *t == ui.settings.context_turns)
                .unwrap_or(1);
            Some((TURNS.iter().map(|(l, _)| l.to_string()).collect(), current))
        }
        // 输出组那个下拉选"生效中"的一条
        "prompt" => {
            let current = ui
                .settings
                .active_prompt
                .min(ui.settings.prompts.len().saturating_sub(1));
            let names: Vec<String> = ui.settings.prompts.iter().map(|p| p.name.clone()).collect();
            Some((names, current))
        }
        // 设置组里的选择器选的是"草稿里第几条"，跟上面那个不是一回事
        "prompt-pick" => {
            let current = ui
                .draft_prompt
                .min(ui.draft_prompts.len().saturating_sub(1));
            let names: Vec<String> = ui.draft_prompts.iter().map(|p| p.name.clone()).collect();
            Some((names, current))
        }
        // 没有兜底：命中表里新加一个 id 而忘了在这里加分支，编译期就要说清楚
        _ => None,
    }
}

/// 把一次选择落到状态上：改设置、必要时重挂上下文、重算 payload。
/// 返回 true 表示要重画。单独拆出来是因为 TrackPopupMenu 是模态的，
/// 无人值守时没法点它 —— 这一段得能脱离菜单被测试。
fn apply_pick(ui: &mut Ui, id: &'static str, index: usize) -> bool {
    match id {
        "agent" => match AGENTS.get(index) {
            Some(agent) => ui.agent = (*agent).to_string(),
            None => return false,
        },
        "turns" => match TURNS.get(index) {
            Some((_, turns)) => ui.settings.context_turns = *turns,
            None => return false,
        },
        "prompt" => {
            if index >= ui.settings.prompts.len() {
                return false;
            }
            ui.settings.active_prompt = index;
        }
        "prompt-pick" => {
            if index >= ui.draft_prompts.len() {
                return false;
            }
            ui.draft_prompt = index;
            ui.dirty = true;
        }
        _ => return false,
    }
    // 有选区才值得重算：没选区时 payload 本来就是 None，重算只会白读一遍磁盘
    if ui.pack.selection.is_some() {
        let settings = ui.settings.clone();
        let agent = ui.agent.clone();
        // 浏览器里已经挑过一行就沿用那一条，否则让 pack 按 agent 自己找
        let picked = ui.browser_sel.and_then(|i| ui.browser_refs.get(i));
        if settings.with_context {
            ui.pack.attach(
                &settings,
                &agent,
                settings.context_turns,
                picked.map(|r| r.file_path.as_str()),
                picked.and_then(|r| r.session_id.as_deref()),
            );
        }
        ui.payload = ui.pack.payload(&settings);
    }
    true
}

unsafe fn pick_menu(id: &'static str) {
    let panel = UI.with(|u| u.borrow().panel);
    if panel.0.is_null() {
        return;
    }
    let Some((items, current)) = UI.with(|u| menu_for(&u.borrow(), id)) else {
        return;
    };
    let menu = match CreatePopupMenu() {
        Ok(m) => m,
        Err(_) => return,
    };
    let mut buffers: Vec<Vec<u16>> = Vec::with_capacity(items.len());
    for (i, text) in items.iter().enumerate() {
        buffers.push(text.encode_utf16().chain(std::iter::once(0)).collect());
        let _ = AppendMenuW(menu, MF_STRING, i + 1, PCWSTR(buffers[i].as_ptr()));
    }
    let _ = CheckMenuItem(menu, (current + 1) as u32, (MF_CHECKED | MF_BYCOMMAND).0);
    // 菜单要挂在光标下，并且必须先让本窗口成为前台，否则点别处不会自动收起
    let mut pt = POINT::default();
    let _ = GetCursorPos(&mut pt);
    let _ = SetForegroundWindow(panel);
    let picked = TrackPopupMenu(
        menu,
        TPM_RETURNCMD | TPM_BOTTOMALIGN | TPM_LEFTBUTTON,
        pt.x,
        pt.y,
        None,
        panel,
        None,
    );
    let _ = DestroyMenu(menu);
    // MSDN 明确要求：菜单关掉后补一条消息，否则下一次点标题区不会先收起菜单
    let _ = PostMessageW(Some(panel), WM_NULL, WPARAM(0), LPARAM(0));
    let index = picked.0;
    if index > 0 && UI.with(|u| apply_pick(&mut u.borrow_mut(), id, index as usize - 1)) {
        // 换草稿指向哪条指令不落到磁盘上，其余三个下拉都是即刻生效的设置
        if id != "prompt-pick" {
            persist_settings();
        } else {
            seed_edits();
        }
        let chip = UI.with(|u| u.borrow().hwnd);
        repaint(chip);
    }
}

fn persist_settings() {
    let settings = UI.with(|u| u.borrow().settings.clone());
    if let Err(e) = store().save(&settings) {
        println!("写设置失败: {e}");
    }
}

/// 开站点。只放 http/https —— 与 Electron 的 site:open 同一个校验，
/// 传任意字符串给 ShellExecuteW 等于把「点一下按钮」变成「执行任意关联程序」
unsafe fn open_site(chip: HWND, index: Option<usize>) {
    let target = UI.with(|u| {
        let ui = u.borrow();
        let sites = if ui.settings.with_context {
            &ui.settings.chat_sites
        } else {
            &ui.settings.direct_sites
        };
        index.and_then(|i| sites.get(i)).cloned()
    });
    let Some(target) = target else { return };
    let url = target.url.trim();
    let scheme = url.split_once("://").map_or("", |(s, _)| s);
    if !scheme.eq_ignore_ascii_case("https") && !scheme.eq_ignore_ascii_case("http") {
        flash(chip, "站点地址不是 http/https");
        return;
    }
    let wide: Vec<u16> = url.encode_utf16().chain(std::iter::once(0)).collect();
    let ok = ShellExecuteW(
        Some(chip),
        w!("open"),
        PCWSTR(wide.as_ptr()),
        None,
        None,
        SW_SHOWNORMAL,
    );
    if (ok.0 as isize) <= 32 {
        flash(chip, "打不开站点");
    }
}

unsafe extern "system" fn panel_wndproc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => {
            let height = unsafe {
                let view = UI.with(|u| panel_view(&u.borrow()));
                let layout = panel::paint(hwnd, &view);
                let want = (layout.height * draw::scale(hwnd)) as i32;
                let mut rc = RECT::default();
                let _ = GetClientRect(hwnd, &mut rc);
                UI.with(|u| u.borrow_mut().panel_layout = layout);
                (want, rc.bottom)
            };
            let _ = ValidateRect(Some(hwnd), None);
            // 每帧都摆一次，包括设置组收起的时候：place 拿到的是这一帧注册过的字段框，
            // 没注册就是 None -> SW_HIDE。只在 settings_open 时调的话，收起设置组
            // 只是不再画那几行，三个原生 EDIT 还浮在原位继续吃键盘 ——
            // 而窗口高度被工作区夹住时根本不会缩，遮不住
            unsafe { edits::place(field_rects(hwnd)) };
            // 内容高度要等画完才知道，所以这一帧之后再调一次尺寸。
            // 夹两件事：高度不超过所在屏的工作区（超出的靠滚动看到），以及装不下时把窗口
            // 往上挪 —— 拖到屏幕下沿外面的 ✕ 和「保存设置」是点不到的
            let s = draw::scale(hwnd);
            let wa = unsafe { work_area_of(hwnd) };
            let mut r = RECT::default();
            let _ = unsafe { GetWindowRect(hwnd, &mut r) };
            let room = (wa.bottom - wa.top).max(240) as f32;
            let want = (height.0 as f32).min(room) as i32;
            let top = r.top.min(wa.bottom - want).max(wa.top);
            UI.with(|u| {
                let mut ui = u.borrow_mut();
                ui.panel_window_h = want as f32 / s;
                let over = (ui.panel_layout.height - ui.panel_window_h).max(0.0);
                ui.scroll = ui.scroll.clamp(0.0, over);
            });
            if (want - height.1).abs() > 1 || top != r.top {
                let _ = SetWindowPos(
                    hwnd,
                    None,
                    r.left,
                    top,
                    (theme::WIDTH * s) as i32,
                    want,
                    SWP_NOACTIVATE | SWP_NOZORDER,
                );
            }
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            let pt = lparam_point(l);
            let (hover, row) = UI.with(|u| {
                let ui = u.borrow();
                let s = draw::scale(hwnd);
                match ui.panel_layout.hit(pt.0 as f32 / s, pt.1 as f32 / s) {
                    Some((id, idx)) => (Some(id), (id == "browser").then_some(idx).flatten()),
                    None => (None, None),
                }
            });
            let changed = UI.with(|u| {
                let mut ui = u.borrow_mut();
                if (ui.panel_hover, ui.panel_row) == (hover, row) {
                    false
                } else {
                    ui.panel_hover = hover;
                    ui.panel_row = row;
                    true
                }
            });
            if changed {
                let _ = unsafe { InvalidateRect(Some(hwnd), None, false) };
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let pt = lparam_point(l);
            let hit = UI.with(|u| {
                let ui = u.borrow();
                let s = draw::scale(hwnd);
                ui.panel_layout.hit(pt.0 as f32 / s, pt.1 as f32 / s)
            });
            if let Some((id, idx)) = hit {
                let chip = UI.with(|u| u.borrow().hwnd);
                panel_action(chip, id, idx);
            }
            let _ = InvalidateRect(Some(hwnd), None, false);
            LRESULT(0)
        }
        WM_TIMER => {
            if w.0 == COPYHOLD {
                UI.with(|u| u.borrow_mut().copied = false);
                let _ = KillTimer(Some(hwnd), COPYHOLD);
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            // 一格 3 行：会话浏览器一行 36 DIP，三行刚好一屏的三分之一
            let step = 3.0 * theme::BROWSE_ROW;
            let dir = ((w.0 >> 16) & 0xFFFF) as i16 as f32 / 120.0;
            UI.with(|u| {
                let mut ui = u.borrow_mut();
                let over =
                    (ui.panel_layout.height - ui.panel_window_h - theme::PAD_BOTTOM).max(0.0);
                ui.scroll = (ui.scroll - dir * step).clamp(0.0, over);
            });
            let _ = InvalidateRect(Some(hwnd), None, false);
            LRESULT(0)
        }
        WM_COMMAND => {
            // 子控件把通知码放在 wParam 高 16 位，lParam 是它自己
            let code = ((w.0 >> 16) & 0xFFFF) as u32;
            if code == EN_CHANGE {
                let child = HWND(l.0 as *mut _);
                if edits::owns(child) {
                    UI.with(|u| u.borrow_mut().dirty = true);
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
            }
            LRESULT(0)
        }
        WM_CTLCOLOREDIT => {
            let hdc = windows::Win32::Graphics::Gdi::HDC(w.0 as *mut _);
            let brush = unsafe { edits::color_field(hdc, theme::FIELD, theme::INK) };
            LRESULT(brush.0 as isize)
        }
        WM_DESTROY => {
            unsafe {
                edits::destroy();
                edits::release_brush();
            }
            UI.with(|u| u.borrow_mut().panel = HWND::default());
            LRESULT(0)
        }
        // 空白处可以拖（对应 CSS 的 -webkit-app-region: drag），控件区留给点击
        WM_NCHITTEST => {
            let pt = client_point(hwnd, l);
            let on_control = UI.with(|u| {
                let ui = u.borrow();
                let s = draw::scale(hwnd);
                ui.panel_layout
                    .hit(pt.0 as f32 / s, pt.1 as f32 / s)
                    .is_some()
            });
            LRESULT(if on_control { HTCLIENT } else { HTCAPTION } as isize)
        }
        _ => DefWindowProcW(hwnd, msg, w, l),
    }
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
                Some("dot") => open_panel(hwnd),
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
                FLASH => {
                    UI.with(|u| {
                        let mut ui = u.borrow_mut();
                        ui.error = None;
                        ui.notice = None;
                    });
                    let _ = KillTimer(Some(hwnd), FLASH);
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
                SAVETICK => {
                    save_position_if_moved(hwnd);
                    unsafe { probe(hwnd) };
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            clip::unwatch(hwnd);
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, w, l),
    }
}

/* ---------- 设置组（M4d）：草稿、保存与那三个 EDIT 的调度 ---------- */

/// 这一帧给三个字段框留好的位置（DIP -> 由 edits::place 自己按 DPI 换算）
fn field_rects(panel: HWND) -> [Option<RectF>; edits::COUNT] {
    let s = draw::scale(panel);
    let _ = s;
    std::array::from_fn(|i| UI.with(|u| u.borrow().panel_layout.rect_of(edits::FIELD_IDS[i])))
}

/// 打开设置组时才建 EDIT，并且只在这时和"换了一条指令"时把文字灌进去。
/// 平时绝不回写：用户在框里打了一半，重画一次就把他的输入吞了，这类 bug 极难查。
unsafe fn toggle_settings(chip: HWND) {
    let open = UI.with(|u| {
        let mut ui = u.borrow_mut();
        ui.settings_open = !ui.settings_open;
        if ui.settings_open {
            // 从"生效中"的那条起步，但必须夹在草稿列表的范围里：
            // 新建两条又没保存、期间删掉一条的话，这里会指到列表外面去，
            // 于是框里是空的、保存又被 get_mut 静默丢掉，用户只看到"已保存"
            ui.draft_prompt = ui
                .settings
                .active_prompt
                .min(ui.draft_prompts.len().saturating_sub(1));
        }
        ui.settings_open
    });
    let panel = UI.with(|u| u.borrow().panel);
    if panel.0.is_null() {
        return;
    }
    if open {
        ensure_edits(panel);
    }
    repaint(chip);
}

/// 设置组展开就得有三个原生 EDIT 挂在**当前这个**面板上。面板收起时它们随窗口一起没了，
/// 而 settings_open 是留着的（mac 同理：折叠再展开，设置组还是开着的），
/// 所以重开面板这条路上也要补一次，否则第一次点「设置」只是把它关掉，什么都没建。
unsafe fn ensure_edits(panel: HWND) {
    if panel.0.is_null() {
        return;
    }
    edits::create(panel, theme::FIELD);
    seed_edits();
}

unsafe fn seed_edits() {
    let ui = UI.with(|u| u.borrow().clone_draft());
    edits::set(edits::TEMPLATE, &ui.0);
    edits::set(edits::SESSIONS, &ui.1);
    edits::set(edits::SITES, &ui.2);
}

/// 当前草稿对应的三份文本：模板 / 会话路径 / 当前模式的站点
impl Ui {
    fn clone_draft(&self) -> (String, String, String) {
        let template = self
            .draft_prompts
            .get(self.draft_prompt)
            .map(|p| p.template.clone())
            .unwrap_or_default();
        let sessions = self
            .settings
            .session_paths
            .iter()
            .map(|p| format!("{}|{}", p.agent, p.path))
            .collect::<Vec<_>>()
            .join(
                "
",
            );
        let slot = usize::from(!self.settings.with_context);
        if self.sites_draft[slot].is_empty() {
            let sites = if self.settings.with_context {
                &self.settings.chat_sites
            } else {
                &self.settings.direct_sites
            };
            (template, sessions, settings::sites_text(sites))
        } else {
            (template, sessions, self.sites_draft[slot].clone())
        }
    }
}

unsafe fn toggle_redact(chip: HWND) {
    let settings = UI.with(|u| {
        let mut ui = u.borrow_mut();
        ui.settings.redact_paths = !ui.settings.redact_paths;
        ui.dirty = true;
        ui.settings.clone()
    });
    if settings.with_context {
        attach_current(&settings);
    } else {
        recompute_payload(&settings);
    }
    repaint(chip);
}

unsafe fn new_prompt(chip: HWND) {
    UI.with(|u| {
        let mut ui = u.borrow_mut();
        // 取一个没被占用的编号：直接 len+1 在删过一条之后会造出两条同名"指令 2"。
        // 查草稿而不是查已保存的那份 —— 用户眼里看到的就是草稿
        let used: Vec<String> = ui.draft_prompts.iter().map(|p| p.name.clone()).collect();
        let n = (1..)
            .find(|k| !used.contains(&format!("指令 {k}")))
            .unwrap_or(1);
        ui.draft_prompts.push(settings::PromptTemplate {
            name: format!("指令 {n}"),
            template: "{selection}".into(),
        });
        ui.draft_prompt = ui.draft_prompts.len() - 1;
        ui.dirty = true;
    });
    seed_edits();
    repaint(chip);
}

unsafe fn del_prompt(chip: HWND) {
    let removed = UI.with(|u| {
        let mut ui = u.borrow_mut();
        if ui.draft_prompts.len() <= 1 {
            return false;
        }
        let at = ui.draft_prompt.min(ui.draft_prompts.len() - 1);
        ui.draft_prompts.remove(at);
        ui.draft_prompt = at.saturating_sub(1);
        ui.dirty = true;
        true
    });
    if !removed {
        // flash 自己也要 borrow_mut，放在上面那个闭包里就是"already mutably borrowed"
        // 直接 panic 穿wndproc —— 进程当场没了。默认设置只有一条指令，
        // 所以"开设置 -> 删除"是开机一分钟内就能撞到的崩溃，mac 那边是先 drop(state) 再提示
        flash(chip, "至少保留一条指令");
        return;
    }
    seed_edits();
    repaint(chip);
}

/// 把三个框的内容落到状态上。单独拎成一个纯函数是为了能测：这一步每一步都在改
/// 那份 Electron 也读的 settings.json，出错的方式是"用户的配置被静默换掉"，不是崩。
///
/// 三条判据都来自 mac 的 save_settings：模板先 trim；名字或模板为空的草稿条目不上盘；
/// 站点整框都是坏行时保留原列表（报的文案是"这些行未生效"，换空了就言行不一致）。
fn apply_save(
    ui: &mut Ui,
    template: &str,
    paths: Vec<settings::SessionPath>,
    sites: Vec<settings::SiteTarget>,
    read: bool,
) {
    let at = ui
        .draft_prompt
        .min(ui.draft_prompts.len().saturating_sub(1));
    ui.draft_prompt = at;
    if let Some(prompt) = ui.draft_prompts.get_mut(at) {
        prompt.template = template.trim().to_string();
    }
    let kept: Vec<settings::PromptTemplate> = ui
        .draft_prompts
        .iter()
        .filter(|p| !p.name.trim().is_empty() && !p.template.trim().is_empty())
        .cloned()
        .collect();
    if !kept.is_empty() {
        ui.draft_prompt = ui.draft_prompt.min(kept.len() - 1);
        ui.draft_prompts = kept.clone();
        ui.settings.prompts = kept;
        ui.settings.active_prompt = ui
            .settings
            .active_prompt
            .min(ui.settings.prompts.len().saturating_sub(1));
    }
    ui.settings.session_paths = paths;
    if !sites.is_empty() {
        if read {
            ui.settings.chat_sites = sites;
        } else {
            ui.settings.direct_sites = sites;
        }
    }
    ui.dirty = false;
}

/// 保存：三个框读回来按行解析，坏行只报行号、不清空其它行；
/// 会话路径整份都是坏行时拒绝保存（那样等于把所有会话源停掉，多半是手滑）。
/// 文案与 mac 的 save_settings 同一套，两版一起改。
unsafe fn save_settings(chip: HWND) {
    let template = edits::get(edits::TEMPLATE);
    let sessions_text = edits::get(edits::SESSIONS);
    let sites_text = edits::get(edits::SITES);
    let (paths, bad_paths) = settings::parse_session_paths(&sessions_text);
    let (sites, bad_sites) = settings::parse_sites(&sites_text);
    if paths.is_empty() {
        flash(chip, "会话路径全部是坏行");
        return;
    }
    let read = UI.with(|u| u.borrow().settings.with_context);
    // 框里的文字留在本模式的草稿槽里：保存不等于丢弃编辑历史，
    // 下一次切回来看到的仍然是自己打的那几行
    UI.with(|u| u.borrow_mut().sites_draft[usize::from(!read)] = sites_text.clone());
    UI.with(|u| apply_save(&mut u.borrow_mut(), &template, paths, sites, read));
    let now = UI.with(|u| u.borrow().settings.clone());
    if let Err(e) = store().save(&now) {
        flash(chip, &format!("保存设置失败：{e}"));
        return;
    }
    let now = UI.with(|u| u.borrow().settings.clone());
    if now.with_context {
        attach_current(&now);
    } else {
        recompute_payload(&now);
    }
    let mut warn = Vec::new();
    if !bad_sites.is_empty() {
        let which = if read {
            "会话解读站点"
        } else {
            "直通站点"
        };
        warn.push(format!("{}第 {} 行", which, join_rows(&bad_sites)));
    }
    if !bad_paths.is_empty() {
        warn.push(format!("会话路径第 {} 行", join_rows(&bad_paths)));
    }
    // 坏行的说法与 mac 一致：说清楚每类行该长什么样，以及"这些行没生效"而不是"整份没保存"
    let message = if warn.is_empty() {
        "已保存".to_string()
    } else {
        format!(
            "无效行（站点需 名称|http(s)://URL，路径需 {}|路径）：{}，未生效",
            settings::AGENT_TOKENS.join("/"),
            warn.join("，")
        )
    };
    flash(chip, &message);
    repaint(chip);
}

fn join_rows(rows: &[usize]) -> String {
    rows.iter()
        .map(|r| r.to_string())
        .collect::<Vec<_>>()
        .join("、")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// thread_local 每线程一份，测试各拿各的 Ui，不会互相踩
    fn with_ui<R>(f: impl FnOnce(&mut Ui) -> R) -> R {
        UI.with(|u| {
            let mut ui = u.borrow_mut();
            // 跟 run() 里读盘之后那一步同构：草稿从已保存的那份起步，
            // 否则测试面对的是一个程序里不会出现的状态
            ui.draft_prompts = ui.settings.prompts.clone();
            ui.draft_prompt = ui
                .settings
                .active_prompt
                .min(ui.draft_prompts.len().saturating_sub(1));
            f(&mut ui)
        })
    }

    #[test]
    fn turns_menu_maps_back_to_the_setting() {
        with_ui(|u| {
            u.settings.context_turns = 16;
            let (items, current) = menu_for(u, "turns").unwrap();
            assert_eq!(items.len(), TURNS.len());
            assert_eq!(items[current], "16 轮");
            assert!(apply_pick(u, "turns", 4));
            assert_eq!(u.settings.context_turns, 0, "「全部」就是 0，由 pack 认");
            assert!(!apply_pick(u, "turns", 99), "越界的选择要被拒掉");
        });
    }

    #[test]
    fn agent_menu_defaults_to_auto() {
        with_ui(|u| {
            let (items, current) = menu_for(u, "agent").unwrap();
            assert_eq!(items[current], "auto");
            assert!(apply_pick(u, "agent", 2));
            assert_eq!(u.agent, "codex");
        });
    }

    /// 没有选区时 apply_pick 只改状态，绝不去读磁盘上的会话
    /// 切到直通必须清掉上下文：否则"直通"复制出去的还是组装后的整段 Prompt
    #[test]
    fn leaving_read_mode_clears_the_context() {
        with_ui(|u| {
            u.settings.with_context = true;
            u.browsing = true;
            u.browser_sel = Some(0);
            assert!(apply_mode(u, false));
            assert!(!u.browsing && u.browser_sel.is_none());
            assert!(u.pack.context.agent.is_none() && u.pack.transcript.is_empty());
            assert!(!apply_mode(u, false), "同一个模式再点一次不该重做任何事");
        });
    }

    /// 设置组的指令选择器只动草稿，不碰已生效的那条，也不落盘
    #[test]
    fn prompt_pick_moves_the_draft_not_the_active_one() {
        with_ui(|u| {
            u.draft_prompts.push(settings::PromptTemplate {
                name: "另一条".into(),
                template: "{selection}!".into(),
            });
            u.settings.active_prompt = 0;
            assert!(apply_pick(u, "prompt-pick", 1));
            assert_eq!(u.draft_prompt, 1);
            assert_eq!(u.settings.active_prompt, 0, "没保存就不该改到生效中的那条");
            assert!(u.dirty);
            assert!(!apply_pick(u, "prompt-pick", 9));
        });
    }

    /// 草稿列表不能越界：越界了 clone_draft 交出空串、apply_save 的 get_mut 静默丢掉，
    /// 用户看到"已保存"而改动没了
    #[test]
    fn the_draft_index_stays_inside_the_draft_list() {
        with_ui(|u| {
            u.draft_prompts.push(settings::PromptTemplate {
                name: "指令 2".into(),
                template: "第二条".into(),
            });
            u.draft_prompt = 1;
            u.draft_prompts.remove(1);
            // 模拟"打开设置组"这一步的夹取
            u.draft_prompt = u
                .settings
                .active_prompt
                .min(u.draft_prompts.len().saturating_sub(1));
            assert_eq!(u.draft_prompt, 0);
            assert_eq!(u.clone_draft().0, u.draft_prompts[0].template);
        });
    }

    /// 站点框整框都是坏行时，保存不能把用户的列表换成空的 ——
    /// 报的文案是"这些行未生效"，换成空列表就是言行不一致，而且这份文件 Electron 也在读
    #[test]
    fn an_unparseable_site_list_does_not_wipe_the_old_one() {
        with_ui(|u| {
            let before = u.settings.chat_sites.clone();
            assert!(!before.is_empty());
            let (sites, bad) = settings::parse_sites("没协议的站点|example.com");
            assert!(sites.is_empty());
            assert_eq!(bad, vec![1]);
            let paths = u.settings.session_paths.clone();
            apply_save(u, "模板", paths, sites, true);
            assert_eq!(u.settings.chat_sites, before, "一行都没解析出来就保留原样");
        });
    }

    /// 保存那一刻才把草稿发布到 settings.prompts，并且空条目不上盘、下标跟着夹
    #[test]
    fn saving_publishes_the_draft_and_drops_empty_entries() {
        with_ui(|u| {
            u.draft_prompts.push(settings::PromptTemplate {
                name: "指令 2".into(),
                template: String::new(),
            });
            u.settings.active_prompt = 9;
            let paths = u.settings.session_paths.clone();
            apply_save(
                u,
                "  带空白的模板  ",
                paths,
                u.settings.chat_sites.clone(),
                true,
            );
            assert_eq!(
                u.draft_prompts.len(),
                1,
                "模板为空的那条不该上盘，也不该留在草稿里"
            );
            assert_eq!(u.settings.prompts.len(), 1);
            assert_eq!(
                u.settings.prompts[0].template, "带空白的模板",
                "跟 mac 一样 trim"
            );
            assert_eq!(
                u.settings.active_prompt, 0,
                "列表短了就要夹回去，不能指着外面"
            );
            assert!(!u.dirty);
        });
    }

    /// 设置组里"指令 N ▾"和"N / 共 M 条"标的是草稿指向的那条，不是生效中的那条。
    /// mac 的 reload_prompt_editor 用 edit_index 同时喂标签和编辑框，这边也必须同源，
    /// 否则上面那个下拉一切换，标签说在编辑指令 3、框里还是指令 1、保存写回指令 1。
    #[test]
    fn the_settings_group_labels_the_draft_it_edits() {
        with_ui(|u| {
            u.settings.prompts.push(settings::PromptTemplate {
                name: "另一条".into(),
                template: "{selection}!".into(),
            });
            u.settings.active_prompt = 1;
            u.draft_prompt = 0;
            assert_eq!(panel_view(u).prompt_index, 0);
            u.draft_prompt = 1;
            assert_eq!(panel_view(u).prompt_index, 1);
        });
    }

    #[test]
    fn picking_without_a_selection_does_no_rework() {
        with_ui(|u| {
            u.payload = None;
            assert!(apply_pick(u, "prompt", 0));
            assert!(u.payload.is_none());
        });
    }
}
