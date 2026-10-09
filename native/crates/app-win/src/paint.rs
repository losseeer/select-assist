//! chip 的绘制。走 GDI 双缓冲：Direct2D 的 HwndRenderTarget 在这台机器上 EndDraw 报成功
//! 却从不合成上屏（同一段代码换成 GDI 立刻可见），而对一个扁平 HUD 来说 GDI 也够用。
//!
//! 尺寸与配色全部来自 crate::theme —— 那张表逐条抄自 mac 侧的 views.rs，
//! 两个平台的小条必须同构，否则切换版本时视觉上会跳一下。

use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, ReleaseDC,
    SelectObject, DT_CENTER, DT_END_ELLIPSIS, HDC, SRCCOPY,
};
use windows::Win32::UI::WindowsAndMessaging::GetClientRect;

use crate::draw::{self, RectF};
use crate::theme::{
    ACCENT, ACCENT_HOVER, CARD, CTRL_H, DIM, DOT, GAP, ICON, PAD_X, R_CTRL, TRACK, T_BODY, WARN,
    WHITE, WIDTH,
};

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

fn layout(hdc: HDC, hwnd: HWND, chip: &Chip, h: f32) -> Layout {
    let btn_w = draw::measure(hdc, hwnd, &chip.button, T_BODY) + 24.0; // CSS 的 padding: 4px 12px
    let top = (h - CTRL_H) / 2.0;
    let btn_left = WIDTH - PAD_X - btn_w;
    Layout {
        button: RectF::new(btn_left, top, btn_w, CTRL_H),
        dot: RectF::new(PAD_X, top, ICON, CTRL_H),
    }
}

/// 画进内存位图再一次性贴上去：尺寸变化时不会闪白底
pub fn paint(hwnd: HWND, chip: &Chip) -> Layout {
    let mut rc = RECT::default();
    unsafe {
        let _ = GetClientRect(hwnd, &mut rc);
    }
    let (pw, ph) = (rc.right.max(1), rc.bottom.max(1));
    let s = draw::scale(hwnd);
    let dh = ph as f32 / s;

    unsafe {
        let sdc = GetDC(Some(hwnd));
        let mem = CreateCompatibleDC(Some(sdc));
        let bmp = CreateCompatibleBitmap(sdc, pw, ph);
        let old = SelectObject(mem, bmp.into());
        draw::select_font(mem, hwnd, T_BODY);

        let l = layout(mem, hwnd, chip, dh);
        draw::fill(mem, rc, CARD);

        // ▸ 带底槽（mac 的 socket_button）：光一个字形读起来不像控件
        let socket = l.dot.to_native(s);
        draw::fill_round(mem, socket, R_CTRL, s, TRACK);
        draw::text(mem, "▸", socket, DIM, DT_CENTER | draw::line());

        // 未读点占的槽位永远留着：亮灭都不动布局，也不会压到按钮文字（同 mac 的 badge_x）
        let status_x = l.dot.right + GAP;
        draw::text(
            mem,
            &chip.status,
            RectF::new(status_x, 0.0, l.button.left - GAP - DOT - status_x, dh).to_native(s),
            if chip.status_err { WARN } else { DIM },
            DT_END_ELLIPSIS | draw::line(),
        );

        if chip.badge {
            let dot = RectF::new(l.button.left - GAP - DOT, (dh - DOT) / 2.0, DOT, DOT);
            draw::fill_ellipse(mem, dot.to_native(s), ACCENT);
        }

        let btn = l.button.to_native(s);
        draw::fill_round(
            mem,
            btn,
            R_CTRL,
            s,
            if chip.hover { ACCENT_HOVER } else { ACCENT },
        );
        draw::text(mem, &chip.button, btn, WHITE, DT_CENTER | draw::line());

        let _ = BitBlt(sdc, 0, 0, pw, ph, Some(mem), 0, 0, SRCCOPY);

        SelectObject(mem, old);
        let _ = DeleteObject(bmp.into());
        let _ = DeleteDC(mem);
        ReleaseDC(Some(hwnd), sdc);
        l
    }
}

pub fn hit(layout: &Layout, pt: (i32, i32), dpi: f32) -> Option<&'static str> {
    let s = 96.0 / dpi;
    let (x, y) = (pt.0 as f32 * s, pt.1 as f32 * s);
    if layout.button.contains(x, y) {
        Some("button")
    } else if layout.dot.contains(x, y) {
        Some("dot")
    } else {
        None
    }
}
