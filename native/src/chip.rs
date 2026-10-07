//! chip 窗口：折叠态的标题栏。
//! Electron 的 `focusable:false` 非激活悬浮 = Borderless（不可为 key）+ NonactivatingPanel。

use objc2::rc::Retained;
use objc2::MainThreadMarker;
use objc2_app_kit::{NSBox, NSPanel, NSTextField, NSWindowStyleMask};

use crate::geo::Geometry;
use crate::views::{self, Pill, DOT, GAP, ICON, LINE_H, PAD_X, R_WINDOW, T_BODY};

pub const WIDTH: f64 = 400.0;
pub const HEIGHT: f64 = 44.0;
/// Electron defaultChipPos()：主屏 workArea 右上角内缩 16 / 60
pub const MARGIN_RIGHT: f64 = 16.0;
pub const MARGIN_TOP: f64 = 60.0;

pub struct Chip {
    pub window: Retained<NSPanel>,
    /// ▸ 展开键：带底槽，读起来是个控件而不是一枚字形（走查 V3）
    pub dot: Pill,
    pub status: Retained<NSTextField>,
    /// 未读小红点：固定 8×8 占位，亮灭都不动布局
    pub badge: Retained<NSBox>,
    pub capture: Pill,
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
            R_WINDOW,
            &views::veil(),
            Some(&views::hairline()),
        ));

        let dot = views::socket_button(
            mtm,
            "\u{25B8}",
            views::rect(PAD_X, (HEIGHT - ICON) / 2.0, ICON, ICON),
        );
        blur.addSubview(dot.view());

        let capture =
            views::primary_pill(mtm, "取入选区", views::rect(0.0, 0.0, 74.0, views::CTRL_H));
        capture.set_tip(Some("把刚才复制的内容取进来"));
        let capture_w = capture.frame().size.width;
        capture.set_frame(views::rect(
            WIDTH - PAD_X - capture_w,
            (HEIGHT - views::CTRL_H) / 2.0,
            capture_w,
            views::CTRL_H,
        ));
        // 未读点固定在按钮左侧的槽位里：亮灭都不动布局，也不会压到按钮文字
        let badge_x = WIDTH - PAD_X - capture_w - GAP - DOT;
        let status_x = PAD_X + ICON + GAP;
        let status = views::label(
            mtm,
            crate::capture::NO_SELECTION,
            T_BODY,
            &views::dim(),
            views::rect(
                status_x,
                (HEIGHT - LINE_H) / 2.0,
                badge_x - status_x - GAP,
                LINE_H,
            ),
        );
        blur.addSubview(&status);

        let badge = views::card(
            mtm,
            views::rect(badge_x, (HEIGHT - DOT) / 2.0, DOT, DOT),
            DOT / 2.0,
            &views::accent(),
            None,
        );
        views::set_dot(&badge, false);
        blur.addSubview(&badge);

        blur.addSubview(capture.view());

        // showInactive()：出现但不激活应用
        window.orderFrontRegardless();
        Self {
            window,
            dot,
            status,
            badge,
            capture,
        }
    }
}
