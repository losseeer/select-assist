//! 设置编辑器那三个多行文本框。
//!
//! 为什么用原生 EDIT 而不是自己画：要打字、要 IME、要选区复制粘贴、要多行滚动 ——
//! 这些自己实现是另一个项目。mac 那边同理用的是 NSTextView。
//! 代价是要把控件染成我们的深色，见 WM_CTLCOLOREDIT 那段。

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateSolidBrush, DeleteObject, SetBkColor, SetTextColor, HBRUSH, HDC,
};
use windows::Win32::UI::Controls::{SetWindowTheme, EM_SETMARGINS, WC_EDITW};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, GetWindowTextLengthW, GetWindowTextW, SendMessageW,
    SetWindowPos, SetWindowTextW, ShowWindow, ES_MULTILINE, ES_WANTRETURN, SWP_NOACTIVATE,
    SWP_NOZORDER, SW_HIDE, SW_SHOW, WINDOW_EX_STYLE, WINDOW_STYLE, WM_SETFONT, WS_CHILD,
    WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
};

use crate::draw::{self, RectF};
use crate::theme;

/// 三个框的下标，顺序与 FIELD_IDS 一致
pub const TEMPLATE: usize = 0;
pub const SESSIONS: usize = 1;
pub const SITES: usize = 2;
pub const COUNT: usize = 3;

/// 面板画完一帧后，把这三个 id 对应的矩形交给 `place`
pub const FIELD_IDS: [&str; COUNT] = ["field-template", "field-sessions", "field-sites"];

thread_local! {
    static EDITS: std::cell::RefCell<[HWND; COUNT]> =
        const { std::cell::RefCell::new([HWND(std::ptr::null_mut()); COUNT]) };
    /// WM_CTLCOLOREDIT 要返回一个**存活期覆盖整条消息处理**的画刷，
    /// 每次现造现删是不行的（GDI 会在我们返回之后就把它销毁）
    static BRUSH: std::cell::Cell<HBRUSH> = const { std::cell::Cell::new(HBRUSH(std::ptr::null_mut())) };
}

fn new_edit(panel: HWND) -> HWND {
    unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            WC_EDITW,
            w!(""),
            // 多行 + 自己吃回车：少了 ES_WANTRETURN，回车会被当成"下一个控件"而不是换行
            WINDOW_STYLE(
                WS_CHILD.0
                    | WS_VISIBLE.0
                    | WS_VSCROLL.0
                    | WS_TABSTOP.0
                    | ES_MULTILINE as u32
                    | ES_WANTRETURN as u32,
            ),
            0,
            0,
            0,
            0,
            Some(panel),
            None,
            None,
            None,
        )
        .unwrap_or_default()
    }
}

/// 建三个 EDIT、换上我们的字体、并关掉视觉样式。
/// 不关视觉样式的话 EDIT 会自己画白底 —— 我们返回的深色画刷只染文字周围那一圈。
pub unsafe fn create(panel: HWND, field_bg: COLORREF) {
    // 看 EDIT 而不是看画刷：面板 WM_DESTROY 把两者一起放掉了，只重建一半的话
    // "重开面板 -> 设置组是空的"，要点两次设置才出来
    if !EDITS.with(|e| e.borrow()[0].0.is_null()) {
        return;
    }
    if BRUSH.with(|b| b.get().0.is_null()) {
        BRUSH.with(|b| b.set(CreateSolidBrush(field_bg)));
    }
    let edits = [new_edit(panel), new_edit(panel), new_edit(panel)];
    for h in edits {
        if h.0.is_null() {
            continue;
        }
        let _ = SetWindowTheme(h, w!(""), w!(""));
        let font = draw::font(panel, theme::T_BODY);
        let _ = SendMessageW(
            h,
            WM_SETFONT,
            Some(WPARAM(font.0 as usize)),
            Some(LPARAM(1)),
        );
        // 文字离边框 8px，跟自绘的胶囊按钮同一档内边距。EC_LEFTMARGIN|EC_RIGHTMARGIN = 3
        let _ = SendMessageW(
            h,
            EM_SETMARGINS,
            Some(WPARAM(3)),
            Some(LPARAM(8 | (8 << 16))),
        );
    }
    EDITS.with(|e| *e.borrow_mut() = edits);
}

pub unsafe fn destroy() {
    for h in EDITS.with(|e| *e.borrow()) {
        if !h.0.is_null() {
            let _ = DestroyWindow(h);
        }
    }
    EDITS.with(|e| *e.borrow_mut() = [HWND(std::ptr::null_mut()); COUNT]);
}

/// 摆到面板这一帧给它们留好的框里。字段框是圆角 8 自绘的，EDIT 是方的，
/// 所以往里缩 1px：露出来的那一圈还是我们的底色，看上去像有描边
pub unsafe fn place(rects: [Option<RectF>; COUNT]) {
    let panel = EDITS.with(|e| e.borrow()[0]);
    let scale = if panel.0.is_null() {
        1.0
    } else {
        draw::scale(panel)
    };
    for (h, rect) in EDITS.with(|e| *e.borrow()).iter().zip(rects) {
        if h.0.is_null() {
            continue;
        }
        match rect {
            Some(r) => {
                let native = r.to_native(scale);
                let _ = ShowWindow(*h, SW_SHOW);
                let _ = SetWindowPos(
                    *h,
                    None,
                    native.left + 1,
                    native.top + 1,
                    native.right - native.left - 2,
                    native.bottom - native.top - 2,
                    SWP_NOACTIVATE | SWP_NOZORDER,
                );
            }
            None => {
                let _ = ShowWindow(*h, SW_HIDE);
            }
        }
    }
}

/// 这条通知是不是我们那三个框发出来的（WM_COMMAND 的 lParam 带子窗口句柄）
pub fn owns(hwnd: HWND) -> bool {
    EDITS.with(|e| e.borrow().contains(&hwnd))
}

/// 我们内部一律用 `\n`（settings.json、解析器、mac 侧都是），控件那一头一律用 `\r\n`。
/// 为什么必须在边界上换算：Win32 的 EDIT 只认 `\r` 是换行，光秃秃一个 `\n` 它不折行 ——
/// 站点列表两行会被挤成 `DeepSeek|https://…/ChatGPT|https://…/` 糊成一行，
/// 用户既看不清也改不动。真实输入那轮才暴露出来：WM_SETTEXT/WM_GETTEXT 两边都原样
/// 保留 `\n`，所以只读文本的测试一直是绿的，只有眼睛会看见。
fn to_control(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\n', "\r\n")
}

fn from_control(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

pub fn set(index: usize, text: &str) {
    let h = EDITS.with(|e| e.borrow()[index]);
    if h.0.is_null() {
        return;
    }
    let wide: Vec<u16> = to_control(text)
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        let _ = SetWindowTextW(h, PCWSTR(wide.as_ptr()));
    }
}

pub fn get(index: usize) -> String {
    let h = EDITS.with(|e| e.borrow()[index]);
    if h.0.is_null() {
        return String::new();
    }
    unsafe {
        let len = GetWindowTextLengthW(h) as usize;
        let mut buf = vec![0u16; len + 1];
        let n = GetWindowTextW(h, &mut buf).max(0) as usize;
        from_control(&String::from_utf16_lossy(&buf[..n.min(len)]))
    }
}

/// WM_CTLCOLOREDIT 的答复：深色底 + 亮字，画刷是 create() 里建的那一枚
pub unsafe fn color_field(hdc: HDC, bg: COLORREF, fg: COLORREF) -> HBRUSH {
    let _ = SetBkColor(hdc, bg);
    let _ = SetTextColor(hdc, fg);
    BRUSH.with(|b| b.get())
}

/// 面板销毁时把画刷也带走，不然每次开合都漏一个 GDI 对象
pub unsafe fn release_brush() {
    let b = BRUSH.with(|b| b.replace(HBRUSH::default()));
    if !b.0.is_null() {
        let _ = DeleteObject(b.into());
    }
}

#[cfg(test)]
mod tests {
    use super::{from_control, to_control};

    #[test]
    fn breaks_become_crlf_on_the_way_in_and_back_on_the_way_out() {
        assert_eq!(to_control("a\nb\nc"), "a\r\nb\r\nc");
        assert_eq!(from_control("a\r\nb\r\nc"), "a\nb\nc");
    }

    #[test]
    fn an_already_crlf_text_is_not_doubled() {
        // 用户在框里按回车，控件给回来的是 \r\n；存进草稿再种回去不能变成 \r\r\n
        assert_eq!(to_control("a\r\nb"), "a\r\nb");
        assert_eq!(from_control(&to_control("a\r\nb")), "a\nb");
    }

    #[test]
    fn a_lone_cr_counts_as_a_break_too() {
        // 粘贴进来的老 Mac 文本
        assert_eq!(from_control("a\rb"), "a\nb");
    }

    #[test]
    fn round_trips_the_settings_shape() {
        let sites = "DeepSeek|https://chat.deepseek.com/\nChatGPT|https://chatgpt.com/";
        assert_eq!(from_control(&to_control(sites)), sites);
        assert_eq!(to_control("").as_str(), "");
    }
}
