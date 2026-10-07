//! panel 窗口：展开态。对应 Electron 的 panelWin（focusable:true，show/hide 而非 resize），
//! 用 Titled + FullSizeContentView 才能成为 key window，NonactivatingPanel 保证显示它时不激活应用。
//! 高度不写死：头部行 + body 里的内容行算出来，M2 的「复制选区原文 / 站点 / 字数」就是头两行。

use objc2::rc::Retained;
use objc2::MainThreadMarker;
use objc2_app_kit::{
    NSBox, NSButton, NSColor, NSPanel, NSTextField, NSView, NSVisualEffectView, NSWindowStyleMask,
    NSWindowTitleVisibility,
};
use objc2_foundation::{NSPoint, NSString};

use crate::geo::Geometry;
use crate::settings::SiteTarget;
use crate::views::{
    self, BADGE, BUTTON_H, BUTTON_W, GAP, HEAD_H, ICON, PAD_BOTTOM, PAD_TOP, PAD_X, RADIUS,
    ROW_GAP, TEXT_H,
};

pub const WIDTH: f64 = 400.0;
/// #actions / #sites 的 flex gap 是 6px（比标题行的 8px 紧一档）
const INNER_GAP: f64 = 6.0;

/// 面板高度 = 上内边距 + 头部行 + Σ(间距 + 内容行) + 下内边距
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
    /// 取入选区按钮右上角的未读点（#head-capture.newclip::after）
    pub unread: Retained<NSBox>,
    body: Retained<NSView>,
    pub copy: Retained<NSButton>,
    pub meta: Retained<NSTextField>,
    pub sites: Vec<Retained<NSButton>>,
}

/// 让按钮按文案自适应，但行高固定，避免换标签时整行跳动
fn fitted(button: &NSButton, x: f64, y: f64) -> f64 {
    button.sizeToFit();
    let width = button.frame().size.width.max(BUTTON_W);
    button.setFrame(views::rect(x, y, width, BUTTON_H));
    width
}

impl Panel {
    /// 初始高度就是内容算出来的高度；anchor 是窗口左上角的全局坐标
    pub fn create(
        mtm: MainThreadMarker,
        geometry: &Geometry,
        anchor: (f64, f64),
        sites: &[SiteTarget],
    ) -> Self {
        let height = height_of(&[BUTTON_H, TEXT_H]);
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
            crate::capture::NO_SELECTION,
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

        let unread = views::card(
            mtm,
            views::rect(0.0, 0.0, BADGE, BADGE),
            BADGE / 2.0,
            &views::rgba(10.0, 132.0, 255.0, 1.0),
            None,
        );
        views::set_dot(&unread, false);
        blur.addSubview(&unread);

        // ---- body 的两行：#actions（复制 + 站点）与 #pack-meta（字数） ----
        let body = NSView::new(mtm);
        blur.addSubview(&body);

        let actions = NSView::new(mtm);
        // 行高要显式给：stack_rows 按子视图高度累加，0 高的容器会让按钮顶到头部行上
        actions.setFrame(views::rect(0.0, 0.0, WIDTH - 2.0 * PAD_X, BUTTON_H));
        body.addSubview(&actions);

        let copy = views::push_button(
            mtm,
            "复制选区原文",
            views::rect(0.0, 0.0, BUTTON_W, BUTTON_H),
            None,
            None,
        );
        actions.addSubview(&copy);
        let mut cursor = fitted(&copy, 0.0, 0.0);

        let mut site_buttons = Vec::with_capacity(sites.len());
        for (index, site) in sites.iter().enumerate() {
            let button = views::text_button(mtm, &site.name, views::rect(0.0, 0.0, 8.0, BUTTON_H));
            button.setToolTip(Some(&NSString::from_str(&site.url)));
            button.setTag(index as isize);
            actions.addSubview(&button);
            cursor += fitted(&button, cursor, 0.0) + INNER_GAP;
            site_buttons.push(button);
        }

        let meta = views::label(
            mtm,
            "",
            12.0,
            &NSColor::secondaryLabelColor(),
            views::rect(0.0, 0.0, WIDTH - 2.0 * PAD_X, TEXT_H),
        );
        body.addSubview(&meta);

        let panel = Self {
            window,
            blur,
            card,
            collapse,
            status,
            capture,
            quit,
            unread,
            body,
            copy,
            meta,
            sites: site_buttons,
        };
        panel.arrange(height);
        panel
    }

    pub fn height(&self) -> f64 {
        height_of(&self.row_heights())
    }

    /// 高度变了要重排：头部行贴顶、body 撑满剩下的空间、圆角遮罩按新尺寸重画
    pub fn arrange(&self, height: f64) {
        let head_bottom = height - PAD_TOP - HEAD_H;
        let icon_y = head_bottom + (HEAD_H - ICON) / 2.0;
        self.card.setFrame(views::rect(0.0, 0.0, WIDTH, height));

        self.collapse
            .setFrame(views::rect(PAD_X, icon_y, ICON, ICON));
        self.quit
            .setFrame(views::rect(WIDTH - PAD_X - ICON, icon_y, ICON, ICON));
        let capture_x = WIDTH - PAD_X - ICON - GAP - BUTTON_W;
        self.capture
            .setFrame(views::rect(capture_x, head_bottom, BUTTON_W, BUTTON_H));
        // top:-3 / right:-3
        self.unread.setFrame(views::rect(
            capture_x + BUTTON_W - BADGE + 3.0,
            head_bottom + BUTTON_H - BADGE + 3.0,
            BADGE,
            BADGE,
        ));
        let status_x = PAD_X + ICON + GAP;
        self.status.setFrame(views::rect(
            status_x,
            head_bottom + (HEAD_H - TEXT_H) / 2.0,
            capture_x - GAP - status_x,
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

    /// M2 的头两行：动作行 24 + 字数行 16
    #[test]
    fn m2_panel_measures_its_two_body_rows() {
        assert_eq!(height_of(&[BUTTON_H, TEXT_H]), 108.0);
        assert_eq!(body_of(&[BUTTON_H, TEXT_H]), 52.0);
    }
}
