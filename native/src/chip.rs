//! M0：chip 窗口的 NSPanel 版，对照 packages/panel/src/main/index.ts 的 chipWin
//! （400×44 / hud 毛玻璃 / 圆角 12 / floating 层 / 点击不夺键盘焦点）

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::Bool;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::*;
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

/// 与 Electron 的 CHIP 常量一致：与面板同宽，展开/折叠只是高度变化
pub const WIDTH: f64 = 400.0;
pub const HEIGHT: f64 = 44.0;

const RADIUS: f64 = 12.0;
const PAD_X: f64 = 12.0;
const GAP: f64 = 8.0;
const ICON: f64 = 22.0;
const BADGE: f64 = 8.0;
const BUTTON_W: f64 = 74.0;
const BUTTON_H: f64 = 24.0;
/// 状态行与「取入选区」按钮的字高
const TEXT_H: f64 = 16.0;
/// Electron defaultChipPos()：主屏 workArea 右上角内缩 16 / 60
const MARGIN_RIGHT: f64 = 16.0;
const MARGIN_TOP: f64 = 60.0;

fn rect(x: f64, y: f64, w: f64, h: f64) -> NSRect {
    NSRect::new(NSPoint::new(x, y), NSSize::new(w, h))
}

fn rgba(r: f64, g: f64, b: f64, a: f64) -> Retained<NSColor> {
    NSColor::colorWithRed_green_blue_alpha(r / 255.0, g / 255.0, b / 255.0, a)
}

/// style.css 的卡片：--bg 遮罩 + --line 发丝描边 + 12pt 圆角
fn card(
    mtm: MainThreadMarker,
    frame: NSRect,
    radius: f64,
    fill: &NSColor,
    border: Option<&NSColor>,
) -> Retained<NSBox> {
    let card = NSBox::new(mtm);
    card.setFrame(frame);
    card.setBoxType(NSBoxType::Custom);
    card.setCornerRadius(radius);
    card.setFillColor(fill);
    card.setTitlePosition(NSTitlePosition::NoTitle);
    match border {
        Some(color) => {
            card.setBorderWidth(1.0);
            card.setBorderColor(color);
        }
        None => card.setBorderWidth(0.0),
    }
    card
}

/// 毛玻璃只能靠 maskImage 变圆角：窗口本身是透明的，方形模糊会在四角露出来
fn rounded_mask(radius: f64) -> Retained<NSImage> {
    let white = NSColor::whiteColor();
    let mask = RcBlock::new(move |rect: NSRect| -> Bool {
        NSRectFillUsingOperation(rect, NSCompositingOperation::Clear);
        white.set();
        NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(rect, radius, radius).fill();
        Bool::YES
    });
    NSImage::imageWithSize_flipped_drawingHandler(NSSize::new(WIDTH, HEIGHT), false, &mask)
}

fn label(
    mtm: MainThreadMarker,
    text: &str,
    size: f64,
    tint: &NSColor,
    frame: NSRect,
) -> Retained<NSTextField> {
    let field = NSTextField::labelWithString(&NSString::from_str(text), mtm);
    field.setFrame(frame);
    field.setFont(Some(&NSFont::systemFontOfSize(size)));
    field.setTextColor(Some(tint));
    field
}

/// 右上角落，与 Electron 版同一位置；M1 才读 settings 里持久化的 windowX/windowY
fn default_origin(mtm: MainThreadMarker) -> NSPoint {
    let visible = NSScreen::mainScreen(mtm).expect("no screen").visibleFrame();
    NSPoint::new(
        visible.origin.x + visible.size.width - WIDTH - MARGIN_RIGHT,
        visible.origin.y + visible.size.height - MARGIN_TOP - HEIGHT,
    )
}

pub fn create(mtm: MainThreadMarker) -> Retained<NSPanel> {
    // Borderless 面板不可为 key，NonactivatingPanel 又不激活应用：合起来即 focusable:false
    let style = NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel;
    let origin = default_origin(mtm);
    let panel = NSPanel::initWithContentRect_styleMask_backing_defer(
        NSPanel::alloc(mtm),
        rect(origin.x, origin.y, WIDTH, HEIGHT),
        style,
        NSBackingStoreType::Buffered,
        false,
    );
    unsafe {
        panel.setReleasedWhenClosed(false);
    }
    // 圆角由 maskImage 与 NSBox 负责，所以窗口自身不画圆角、也不画背景
    panel.setOpaque(false);
    panel.setBackgroundColor(Some(&NSColor::clearColor()));
    panel.setHasShadow(true);
    panel.setLevel(NSFloatingWindowLevel);
    // 等价于 alwaysOnTop + setVisibleOnAllWorkspaces({ visibleOnFullScreen: true })
    panel.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::FullScreenAuxiliary,
    );
    let vibrant_dark = NSAppearance::appearanceNamed(unsafe { NSAppearanceNameVibrantDark });
    panel.setAppearance(vibrant_dark.as_deref());

    let blur = NSVisualEffectView::initWithFrame(
        NSVisualEffectView::alloc(mtm),
        rect(0.0, 0.0, WIDTH, HEIGHT),
    );
    blur.setMaterial(NSVisualEffectMaterial::HUDWindow);
    blur.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
    // 桌面/Finder 拿到焦点时也保持 active 态模糊，等同 visualEffectState: 'active'
    blur.setState(NSVisualEffectState::Active);
    blur.setMaskImage(Some(&rounded_mask(RADIUS)));
    panel.setContentView(Some(&blur));

    blur.addSubview(&card(
        mtm,
        rect(0.0, 0.0, WIDTH, HEIGHT),
        RADIUS,
        &rgba(22.0, 22.0, 24.0, 0.4),
        Some(&rgba(255.0, 255.0, 255.0, 0.10)),
    ));

    let dot = label(
        mtm,
        "\u{25B8}",
        12.0,
        &NSColor::secondaryLabelColor(),
        rect(PAD_X, (HEIGHT - ICON) / 2.0, ICON, ICON),
    );
    dot.setAlignment(NSTextAlignment::Center);
    blur.addSubview(&dot);

    let status_x = PAD_X + ICON + GAP;
    let badge_x = WIDTH - PAD_X - BUTTON_W - GAP - BADGE;
    blur.addSubview(&label(
        mtm,
        "还没有选区",
        12.0,
        &NSColor::secondaryLabelColor(),
        rect(
            status_x,
            (HEIGHT - TEXT_H) / 2.0,
            badge_x - status_x - GAP,
            TEXT_H,
        ),
    ));

    // M0 只是占位的小红点，剪贴板轮询要到 M2 才点亮它；固定尺寸，亮灭都不动布局
    let badge = card(
        mtm,
        rect(badge_x, (HEIGHT - BADGE) / 2.0, BADGE, BADGE),
        BADGE / 2.0,
        &NSColor::systemBlueColor(),
        None,
    );
    badge.setAlphaValue(0.0);
    blur.addSubview(&badge);

    let capture = NSButton::new(mtm);
    capture.setFrame(rect(
        WIDTH - PAD_X - BUTTON_W,
        (HEIGHT - BUTTON_H) / 2.0,
        BUTTON_W,
        BUTTON_H,
    ));
    capture.setBezelStyle(NSBezelStyle::Push);
    capture.setBezelColor(Some(&rgba(10.0, 132.0, 255.0, 1.0)));
    capture.setTitle(&NSString::from_str("取入选区"));
    capture.setFont(Some(&NSFont::systemFontOfSize(12.0)));
    blur.addSubview(&capture);

    // showInactive()：出现但不激活应用
    panel.orderFrontRegardless();
    panel
}
