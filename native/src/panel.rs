//! panel 窗口：展开态。对应 Electron 的 panelWin（focusable:true，show/hide 而非 resize），
//! 用 Titled + FullSizeContentView 才能成为 key window，NonactivatingPanel 保证显示它时不激活应用。

use objc2::rc::Retained;
use objc2::MainThreadMarker;
use objc2_app_kit::{
    NSBox, NSButton, NSColor, NSPanel, NSTextField, NSView, NSVisualEffectView, NSWindowStyleMask,
    NSWindowTitleVisibility,
};
use objc2_foundation::NSPoint;

use crate::geo::Geometry;
use crate::views::{
    self, BUTTON_H, BUTTON_W, HEAD_H, ICON, PAD_BOTTOM, PAD_TOP, PAD_X, RADIUS, ROW_GAP, TEXT_H,
};

pub const WIDTH: f64 = 400.0;

/// 面板高度 = 上内边距 + 头部行 + Σ(间距 + 内容行) + 下内边距。
/// M4 往 body 里加行，这里就自动长，不需要第二个尺寸常量。
fn height_of(rows: &[f64]) -> f64 {
    PAD_TOP + HEAD_H + rows.iter().map(|row| ROW_GAP + row).sum::<f64>() + PAD_BOTTOM
}

/// body 能用的净高度：头部行下面还要留一个 gap
fn body_of(rows: &[f64]) -> f64 {
    height_of(rows) - PAD_TOP - HEAD_H - ROW_GAP - PAD_BOTTOM
}

pub struct Panel {
    pub window: Retained<NSPanel>,
    blur: Retained<NSVisualEffectView>,
    card: Retained<NSBox>,
    pub collapse: Retained<NSButton>,
    pub status: Retained<NSTextField>,
    pub capture: Retained<NSButton>,
    pub quit: Retained<NSButton>,
    body: Retained<NSView>,
}

impl Panel {
    /// 初始高度就是内容算出来的高度；anchor 是窗口左上角的全局坐标
    pub fn create(mtm: MainThreadMarker, geometry: &Geometry, anchor: (f64, f64)) -> Self {
        let height = height_of(&[]);
        let window = views::panel(
            mtm,
            WIDTH,
            height,
            geometry.cocoa_origin(anchor.0, anchor.1, height),
            NSWindowStyleMask::Titled
                | NSWindowStyleMask::FullSizeContentView
                | NSWindowStyleMask::NonactivatingPanel,
        );
        // 有标题栏才能成为 key window，所以把标题栏整条藏掉
        window.setTitlebarAppearsTransparent(true);
        window.setTitleVisibility(NSWindowTitleVisibility::Hidden);

        let blur = views::blur(mtm, WIDTH, height);
        views::set_mask(&blur, WIDTH, height);
        window.setContentView(Some(&blur));

        let card = views::card(
            mtm,
            views::rect(0.0, 0.0, WIDTH, height),
            RADIUS,
            &views::rgba(22.0, 22.0, 24.0, 0.4),
            Some(&views::rgba(255.0, 255.0, 255.0, 0.10)),
        );
        blur.addSubview(&card);

        let collapse = views::glyph_button(
            mtm,
            "\u{25BE}",
            views::rect(0.0, 0.0, ICON, ICON),
            None,
            None,
        );
        blur.addSubview(&collapse);

        let status = views::label(
            mtm,
            "还没有选区",
            12.0,
            &NSColor::secondaryLabelColor(),
            views::rect(0.0, 0.0, 1.0, TEXT_H),
        );
        blur.addSubview(&status);

        let capture = views::push_button(
            mtm,
            "取入选区",
            views::rect(0.0, 0.0, BUTTON_W, BUTTON_H),
            None,
            None,
        );
        blur.addSubview(&capture);

        let quit = views::glyph_button(
            mtm,
            "\u{2715}",
            views::rect(0.0, 0.0, ICON, ICON),
            None,
            None,
        );
        blur.addSubview(&quit);

        let body = NSView::new(mtm);
        blur.addSubview(&body);

        let panel = Self {
            window,
            blur,
            card,
            collapse,
            status,
            capture,
            quit,
            body,
        };
        panel.arrange(height);
        panel
    }

    pub fn height(&self) -> f64 {
        height_of(&self.row_heights())
    }

    /// #panel-head：▾ / 状态 / 取入选区 / ✕，整行贴顶
    pub fn arrange(&self, height: f64) {
        let head_bottom = height - PAD_TOP - HEAD_H;
        let center = head_bottom + (HEAD_H - ICON) / 2.0;
        self.card.setFrame(views::rect(0.0, 0.0, WIDTH, height));

        self.collapse
            .setFrame(views::rect(PAD_X, center, ICON, ICON));
        self.quit
            .setFrame(views::rect(WIDTH - PAD_X - ICON, center, ICON, ICON));
        self.capture.setFrame(views::rect(
            WIDTH - PAD_X - ICON - views::GAP - BUTTON_W,
            head_bottom,
            BUTTON_W,
            BUTTON_H,
        ));
        let status_x = PAD_X + ICON + views::GAP;
        let status_right = WIDTH - PAD_X - ICON - views::GAP - BUTTON_W - views::GAP;
        self.status.setFrame(views::rect(
            status_x,
            head_bottom + (HEAD_H - TEXT_H) / 2.0,
            status_right - status_x,
            TEXT_H,
        ));

        let body_height = body_of(&self.row_heights()).max(0.0);
        self.body.setFrame(views::rect(
            PAD_X,
            PAD_BOTTOM,
            WIDTH - 2.0 * PAD_X,
            body_height,
        ));
        self.stack_rows(body_height);
        views::set_mask(&self.blur, WIDTH, height);
    }

    fn row_heights(&self) -> Vec<f64> {
        let rows = self.body.subviews();
        (0..rows.count())
            .map(|index| rows.objectAtIndex(index).frame().size.height)
            .collect()
    }

    /// body 里的行从上往下排（Cocoa 的 y 向上，所以从可用高度倒着减）
    fn stack_rows(&self, body_height: f64) {
        let rows = self.body.subviews();
        let mut top = body_height;
        for index in 0..rows.count() {
            let row = rows.objectAtIndex(index);
            let h = row.frame().size.height;
            top -= h;
            row.setFrameOrigin(NSPoint::new(0.0, top));
            top -= ROW_GAP;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_panel_is_a_head_row_tall() {
        assert_eq!(height_of(&[]), 44.0);
        assert_eq!(body_of(&[]), -12.0);
    }

    #[test]
    fn every_row_grows_the_panel_by_its_height_plus_a_gap() {
        assert_eq!(height_of(&[24.0]), 80.0);
        assert_eq!(body_of(&[24.0]), 24.0);
        assert_eq!(height_of(&[24.0, 40.0]), 132.0);
        assert_eq!(body_of(&[24.0, 40.0]), 76.0);
    }
}
