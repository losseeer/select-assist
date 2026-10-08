//! GDI 绘制原语，chip 与 panel 共用。
//!
//! 全部按 DIP（96dpi 下的像素）给坐标，函数内部按窗口 DPI 换算 —— 一个 400 宽的 HUD
//! 在 150% 屏上必须是 600 物理像素，否则文字会小得读不动。
//! 圆角交给 DWM（DWMWCP_ROUND），所以这里只画矩形和圆角矩形，不做窗口级的抗锯齿。

use std::cell::RefCell;

use windows::core::w;
use windows::Win32::Foundation::{COLORREF, HWND, RECT, SIZE};
use windows::Win32::Graphics::Gdi::{
    CreateEllipticRgn, CreateFontW, CreateRoundRectRgn, CreateSolidBrush, DeleteObject, DrawTextW,
    FillRect, FillRgn, GetTextExtentPoint32W, SelectObject, SetBkMode, SetTextColor,
    CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DRAW_TEXT_FORMAT, DT_NOPREFIX,
    DT_SINGLELINE, DT_VCENTER, HDC, HFONT, OUT_DEFAULT_PRECIS, TRANSPARENT,
};
use windows::Win32::UI::HiDpi::GetDpiForWindow;

#[derive(Clone, Copy, Default, Debug)]
pub struct RectF {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl RectF {
    pub fn new(left: f32, top: f32, width: f32, height: f32) -> Self {
        Self {
            left,
            top,
            right: left + width,
            bottom: top + height,
        }
    }

    pub fn width(self) -> f32 {
        self.right - self.left
    }

    pub fn height(self) -> f32 {
        self.bottom - self.top
    }

    pub fn contains(self, x: f32, y: f32) -> bool {
        x >= self.left && x <= self.right && y >= self.top && y <= self.bottom
    }

    /// DIP -> 物理像素
    pub fn to_native(self, s: f32) -> RECT {
        RECT {
            left: (self.left * s) as i32,
            top: (self.top * s) as i32,
            right: (self.right * s) as i32,
            bottom: (self.bottom * s) as i32,
        }
    }
}

pub fn scale(hwnd: HWND) -> f32 {
    (unsafe { GetDpiForWindow(hwnd) }) as f32 / 96.0
}

thread_local! {
    /// 字号(pt) -> HFONT。窗口都是 per-monitor v2，同一进程里字号就那三档，够用
    static FONTS: RefCell<Vec<(i32, HFONT)>> = const { RefCell::new(Vec::new()) };
}

/// 字号对应的 HFONT。原生子控件要靠 WM_SETFONT 拿同一张表，不能只在我们自己的 DC 里选
pub fn font(hwnd: HWND, pt: f32) -> HFONT {
    let height = -(pt * scale(hwnd)) as i32;
    FONTS.with(|f| {
        let mut fonts = f.borrow_mut();
        if let Some((_, existing)) = fonts.iter().find(|(h, _)| *h == height) {
            return *existing;
        }
        let created = unsafe {
            CreateFontW(
                height,
                0,
                0,
                0,
                400, // FW_NORMAL
                0,
                0,
                0,
                DEFAULT_CHARSET,
                OUT_DEFAULT_PRECIS,
                CLIP_DEFAULT_PRECIS,
                CLEARTYPE_QUALITY,
                0, // DEFAULT_PITCH | FF_DONTCARE
                w!("Segoe UI"),
            )
        };
        fonts.push((height, created));
        created
    })
}

/// 选中字号对应的字体。内存 DC 用完就销毁，所以不需要还原旧字体
pub fn select_font(hdc: HDC, hwnd: HWND, pt: f32) {
    unsafe {
        let _ = SelectObject(hdc, font(hwnd, pt).into());
    }
}

/// 文本宽度（DIP）。选中的字体已经按 DPI 放大，所以量出来的是物理像素，再除回去。
/// 字体没选对时结果也错 —— 调用前必须 select_font 同一个 pt。
pub fn measure(hdc: HDC, s: &str, pt: f32, scale: f32) -> f32 {
    if s.is_empty() {
        return 0.0;
    }
    let u: Vec<u16> = s.encode_utf16().collect();
    let mut sz = SIZE::default();
    unsafe {
        if GetTextExtentPoint32W(hdc, &u, &mut sz).as_bool() {
            sz.cx as f32 / scale
        } else {
            s.chars().count() as f32 * pt * 0.62
        }
    }
}

pub fn fill(hdc: HDC, r: RECT, color: COLORREF) {
    unsafe {
        let b = CreateSolidBrush(color);
        FillRect(hdc, &r, b);
        let _ = DeleteObject(b.into());
    }
}

pub fn fill_round(hdc: HDC, r: RECT, radius_dip: f32, s: f32, color: COLORREF) {
    let d = (radius_dip * s) as i32 * 2;
    unsafe {
        let rg = CreateRoundRectRgn(r.left, r.top, r.right + 1, r.bottom + 1, d, d);
        let b = CreateSolidBrush(color);
        let _ = FillRgn(hdc, rg, b);
        let _ = DeleteObject(b.into());
        let _ = DeleteObject(rg.into());
    }
}

pub fn fill_ellipse(hdc: HDC, r: RECT, color: COLORREF) {
    unsafe {
        let rg = CreateEllipticRgn(r.left, r.top, r.right, r.bottom);
        let b = CreateSolidBrush(color);
        let _ = FillRgn(hdc, rg, b);
        let _ = DeleteObject(b.into());
        let _ = DeleteObject(rg.into());
    }
}

/// 空串必须挡掉：Vec 为空时 as_ptr 是悬垂的，而 windows-rs 把 len() 当作字符数交给
/// GDI，DrawTextW 在带 DT_RIGHT / DT_END_ELLIPSIS 时会去量这段文字，直接踩到非法地址
/// （面板的 ctx 状态行平时就是空的，第一次画就崩在这里）。
pub fn text(hdc: HDC, s: &str, mut rc: RECT, color: COLORREF, flags: DRAW_TEXT_FORMAT) {
    if s.is_empty() {
        return;
    }
    let mut u: Vec<u16> = s.encode_utf16().collect();
    unsafe {
        let _ = SetTextColor(hdc, color);
        let _ = SetBkMode(hdc, TRANSPARENT);
        let _ = DrawTextW(hdc, &mut u, &mut rc, flags);
    }
}

/// 一行竖排居中的文字，最常用的组合。bitflags 的 `|` 不是 const fn，所以做成函数。
pub fn line() -> DRAW_TEXT_FORMAT {
    DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX
}
