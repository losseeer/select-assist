//! chip 窗口：折叠态的标题栏。
//! Electron 的 `focusable:false` 非激活悬浮 = Borderless（不可为 key）+ NonactivatingPanel。

use objc2::rc::Retained;
use objc2::MainThreadMarker;
use objc2_app_kit::{NSButton, NSColor, NSPanel, NSWindowStyleMask};

use crate::geo::Geometry;
use crate::views::{self, BADGE, BUTTON_H, BUTTON_W, GAP, ICON, PAD_X, RADIUS, TEXT_H};

pub const WIDTH: f64 = 400.0;
pub const HEIGHT: f64 = 44.0;
/// Electron defaultChipPos()：主屏 workArea 右上角内缩 16 / 60
pub const MARGIN_RIGHT: f64 = 16.0;
pub const MARGIN_TOP: f64 = 60.0;

pub struct Chip {
    pub window: Retained<NSPanel>,
    /// ▸ 是展开键（chip 其余控件在 M1 还是静态的，不必留引用）
    pub dot: Retained<NSButton>,
}

impl Chip {
    /// anchor 是窗口左上角的全局坐标（与 settings.json 同一约定），内部换算成 Cocoa 的左下角
    pub fn create(mtm: MainThreadMarker, geometry: &Geometry, anchor: (f64, f64)) -> Self {
        let origin = geometry.cocoa_origin(anchor.0, anchor.1, HEIGHT);
        let window = views::panel(
            mtm,
            WIDTH,
            HEIGHT,
            origin,
            NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel,
        );
        let blur = views::blur(mtm, WIDTH, HEIGHT);
        views::set_mask(&blur, WIDTH, HEIGHT);
        window.setContentView(Some(&blur));

        blur.addSubview(&views::card(
            mtm,
            views::rect(0.0, 0.0, WIDTH, HEIGHT),
            RADIUS,
            &views::rgba(22.0, 22.0, 24.0, 0.4),
            Some(&views::rgba(255.0, 255.0, 255.0, 0.10)),
        ));

        let dot = views::glyph_button(
            mtm,
            "\u{25B8}",
            views::rect(PAD_X, (HEIGHT - ICON) / 2.0, ICON, ICON),
            None,
            None,
        );
        blur.addSubview(&dot);

        let capture_x = WIDTH - PAD_X - BUTTON_W;
        let badge_x = capture_x - GAP - BADGE;
        let status_x = PAD_X + ICON + GAP;
        let status = views::label(
            mtm,
            "还没有选区",
            12.0,
            &NSColor::secondaryLabelColor(),
            views::rect(
                status_x,
                (HEIGHT - TEXT_H) / 2.0,
                badge_x - status_x - GAP,
                TEXT_H,
            ),
        );
        blur.addSubview(&status);

        // 固定占位的小红点：亮灭都不动布局（剪贴板轮询在 M2）
        let badge = views::card(
            mtm,
            views::rect(badge_x, (HEIGHT - BADGE) / 2.0, BADGE, BADGE),
            BADGE / 2.0,
            &NSColor::systemBlueColor(),
            None,
        );
        badge.setAlphaValue(0.0);
        blur.addSubview(&badge);

        let capture = views::push_button(
            mtm,
            "取入选区",
            views::rect(capture_x, (HEIGHT - BUTTON_H) / 2.0, BUTTON_W, BUTTON_H),
            None,
            None,
        );
        blur.addSubview(&capture);

        // showInactive()：出现但不激活应用
        window.orderFrontRegardless();
        Self { window, dot }
    }
}
