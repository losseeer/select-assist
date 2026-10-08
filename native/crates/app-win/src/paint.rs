//! chip 的绘制。走 GDI 双缓冲：Direct2D 的 HwndRenderTarget 在这台机器上 EndDraw 报成功
//! 却从不合成上屏（同一段代码换成 GDI 立刻可见），而对一个扁平 HUD 来说 GDI 也够用。
//! 圆角交给 DWM（DWMWCP_ROUND），所以这里只画矩形，不用自己抗锯齿。
//!
//! 布局用 DIP（96dpi 下的像素），绘制前按窗口 DPI 换算成物理像素；
//! 颜色取自 static/style.css 的实测合成值。

use std::cell::OnceCell;

use windows::core::w;
use windows::Win32::Foundation::{COLORREF, HWND, RECT, SIZE};
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreateEllipticRgn, CreateFontW,
    CreateRoundRectRgn, CreateSolidBrush, DeleteDC, DeleteObject, DrawTextW, FillRect, FillRgn,
    GetDC, GetTextExtentPoint32W, ReleaseDC, SelectObject, SetBkMode, SetTextColor,
    CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DRAW_TEXT_FORMAT, DT_CENTER,
    DT_END_ELLIPSIS, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, FW_NORMAL, HDC, HFONT,
    OUT_DEFAULT_PRECIS, SRCCOPY, TRANSPARENT,
};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::GetClientRect;

// style.css --bg rgba(22,22,24,.4) 叠在无材质窗口上的实测色 (9,9,10)
const CARD: COLORREF = COLORREF(0x000A_0909);
// .status 用的是 --dim = rgba(235,235,245,.6)，合成到 CARD 上：0.6*235+0.4*9 ≈ 145
const DIM: COLORREF = COLORREF(0x0097_9191);
const ACCENT: COLORREF = COLORREF(0x00FF_840A); // #0A84FF，GDI 按 BGR 排
const ACCENT_HOVER: COLORREF = COLORREF(0x00FF_9419);
const WARN: COLORREF = COLORREF(0x003C_A1E5); // --warn #e5a13c，状态行报错时换成它
const WHITE: COLORREF = COLORREF(0x00FF_FFFF);

const PAD_X: f32 = 12.0;
const GAP: f32 = 8.0;
const DOT_W: f32 = 12.0;
const BADGE_W: f32 = 8.0;
const BTN_H: f32 = 24.0;
const FONT_SIZE: f32 = 12.0;

#[derive(Clone, Copy, Default)]
pub struct RectF {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl RectF {
    fn to_native(self, s: f32) -> RECT {
        RECT {
            left: (self.left * s) as i32,
            top: (self.top * s) as i32,
            right: (self.right * s) as i32,
            bottom: (self.bottom * s) as i32,
        }
    }
}

pub struct Chip {
    pub status: String,
    /// 状态行是不是错误提示（对应 .status.err，只换颜色）
    pub status_err: bool,
    pub badge: bool,
    pub button: String,
    pub hover: bool,
}

impl Default for Chip {
    fn default() -> Self {
        Self {
            status: capture::NO_SELECTION.into(),
            status_err: false,
            badge: false,
            button: "取入选区".into(),
            hover: false,
        }
    }
}

/// 绘制与命中判定共用同一份布局（DIP 坐标），避免两处算歪
#[derive(Clone, Copy, Default)]
pub struct Layout {
    pub button: RectF,
    pub dot: RectF,
}

thread_local! {
    static FONT: OnceCell<HFONT> = const { OnceCell::new() };
}

fn font(hwnd: HWND) -> HFONT {
    FONT.with(|f| {
        *f.get_or_init(|| unsafe {
            let dpi = GetDpiForWindow(hwnd) as f32;
            CreateFontW(
                -(FONT_SIZE * dpi / 96.0) as i32,
                0,
                0,
                0,
                FW_NORMAL.0 as i32,
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
        })
    })
}

fn measure(hdc: HDC, s: &str) -> f32 {
    let u: Vec<u16> = s.encode_utf16().collect();
    let mut sz = SIZE::default();
    unsafe {
        if GetTextExtentPoint32W(hdc, &u, &mut sz).as_bool() {
            sz.cx as f32
        } else {
            s.chars().count() as f32 * FONT_SIZE * 0.62
        }
    }
}

fn layout(hdc: HDC, chip: &Chip, w: f32, h: f32) -> Layout {
    let btn_w = measure(hdc, &chip.button) + 24.0; // CSS 的 padding: 4px 12px
    let top = (h - BTN_H) / 2.0;
    let right = w - PAD_X;
    let btn_left = right - btn_w;
    Layout {
        button: RectF {
            left: btn_left,
            top,
            right,
            bottom: top + BTN_H,
        },
        dot: RectF {
            left: PAD_X,
            top,
            right: PAD_X + DOT_W,
            bottom: top + BTN_H,
        },
    }
}

/// 画进内存位图再一次性贴上去：尺寸变化时不会闪白底
pub fn paint(hwnd: HWND, chip: &Chip) -> Layout {
    let mut rc = RECT::default();
    unsafe {
        let _ = GetClientRect(hwnd, &mut rc);
    }
    let (pw, ph) = (rc.right.max(1), rc.bottom.max(1));
    let dpi = unsafe { GetDpiForWindow(hwnd) } as f32;
    let s = dpi / 96.0;

    unsafe {
        let sdc = GetDC(Some(hwnd));
        let mem = CreateCompatibleDC(Some(sdc));
        let bmp = CreateCompatibleBitmap(sdc, pw, ph);
        let old_bmp = SelectObject(mem, bmp.into());
        let old_font = SelectObject(mem, font(hwnd).into());
        let _ = SetBkMode(mem, TRANSPARENT);

        let l = layout(mem, chip, pw as f32 / s, ph as f32 / s);
        fill_rect(mem, pw, ph, CARD);

        let _ = SetTextColor(mem, DIM);
        draw_text(
            mem,
            "▸",
            l.dot.to_native(s),
            DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
        );

        // 未读点占的槽位永远留着：亮灭都不动布局，也不会压到按钮文字（同 mac 的 badge_x）
        let status_right = l.button.left - GAP - BADGE_W;
        let _ = SetTextColor(mem, if chip.status_err { WARN } else { DIM });
        draw_text(
            mem,
            &chip.status,
            RectF {
                left: l.dot.right + GAP,
                top: 0.0,
                right: status_right,
                bottom: ph as f32 / s,
            }
            .to_native(s),
            DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX,
        );

        if chip.badge {
            let r = RectF {
                left: l.button.left - GAP - BADGE_W,
                top: (ph as f32 / s - BADGE_W) / 2.0,
                right: l.button.left - GAP,
                bottom: (ph as f32 / s + BADGE_W) / 2.0,
            };
            fill_ellipse(mem, r.to_native(s), ACCENT);
        }

        let btn = l.button.to_native(s);
        let corner = (6.0 * s) as i32 * 2;
        fill_round_rect(
            mem,
            btn,
            corner,
            corner,
            if chip.hover { ACCENT_HOVER } else { ACCENT },
        );
        let _ = SetTextColor(mem, WHITE);
        draw_text(
            mem,
            &chip.button,
            btn,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
        );

        let _ = BitBlt(sdc, 0, 0, pw, ph, Some(mem), 0, 0, SRCCOPY);

        SelectObject(mem, old_font);
        SelectObject(mem, old_bmp);
        let _ = DeleteObject(bmp.into());
        let _ = DeleteDC(mem);
        ReleaseDC(Some(hwnd), sdc);
        l
    }
}

pub fn hit(layout: &Layout, pt: (i32, i32), dpi: f32) -> Option<&'static str> {
    let s = 96.0 / dpi;
    let x = pt.0 as f32 * s;
    let y = pt.1 as f32 * s;
    let inside = |r: RectF| x >= r.left && x <= r.right && y >= r.top && y <= r.bottom;
    if inside(layout.button) {
        Some("button")
    } else if inside(layout.dot) {
        Some("dot")
    } else {
        None
    }
}

unsafe fn fill_rect(hdc: HDC, w: i32, h: i32, color: COLORREF) {
    let b = CreateSolidBrush(color);
    FillRect(
        hdc,
        &RECT {
            left: 0,
            top: 0,
            right: w,
            bottom: h,
        },
        b,
    );
    let _ = DeleteObject(b.into());
}

unsafe fn fill_round_rect(hdc: HDC, r: RECT, rx: i32, ry: i32, color: COLORREF) {
    let rg = CreateRoundRectRgn(r.left, r.top, r.right + 1, r.bottom + 1, rx, ry);
    let b = CreateSolidBrush(color);
    let _ = FillRgn(hdc, rg, b);
    let _ = DeleteObject(b.into());
    let _ = DeleteObject(rg.into());
}

unsafe fn fill_ellipse(hdc: HDC, r: RECT, color: COLORREF) {
    let rg = CreateEllipticRgn(r.left, r.top, r.right, r.bottom);
    let b = CreateSolidBrush(color);
    let _ = FillRgn(hdc, rg, b);
    let _ = DeleteObject(b.into());
    let _ = DeleteObject(rg.into());
}

unsafe fn draw_text(hdc: HDC, text: &str, mut rc: RECT, flags: DRAW_TEXT_FORMAT) {
    let mut u: Vec<u16> = text.encode_utf16().collect();
    let _ = DrawTextW(hdc, &mut u, &mut rc, flags);
}
