//! 两个窗口共用的视图构造：HUD 毛玻璃 + 圆角遮罩 + 卡片 + 文本/按钮。
//! 数值全部对齐 packages/panel/static/style.css 的设计令牌。

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool};
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::*;
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

pub const RADIUS: f64 = 12.0;
/// #chip / #panel 的 padding: 8px 12px 12px
pub const PAD_TOP: f64 = 8.0;
pub const PAD_X: f64 = 12.0;
pub const PAD_BOTTOM: f64 = 12.0;
/// flex gap
pub const GAP: f64 = 8.0;
pub const ROW_GAP: f64 = 12.0;
/// .icon-btn 22×22、button min-height 24、.status 行高 16
pub const ICON: f64 = 22.0;
pub const HEAD_H: f64 = 24.0;
pub const BUTTON_W: f64 = 74.0;
pub const BUTTON_H: f64 = 24.0;
pub const TEXT_H: f64 = 16.0;
pub const BADGE: f64 = 8.0;

pub fn rect(x: f64, y: f64, w: f64, h: f64) -> NSRect {
    NSRect::new(NSPoint::new(x, y), NSSize::new(w, h))
}

pub fn rgba(r: f64, g: f64, b: f64, a: f64) -> Retained<NSColor> {
    NSColor::colorWithRed_green_blue_alpha(r / 255.0, g / 255.0, b / 255.0, a)
}

/// --bg 遮罩 + 可选的 --line 发丝描边 + 12pt 圆角
pub fn card(
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

pub fn label(
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

/// vibrancy:'hud' + visualEffectState:'active'
pub fn blur(mtm: MainThreadMarker, width: f64, height: f64) -> Retained<NSVisualEffectView> {
    let view = NSVisualEffectView::initWithFrame(
        NSVisualEffectView::alloc(mtm),
        rect(0.0, 0.0, width, height),
    );
    view.setMaterial(NSVisualEffectMaterial::HUDWindow);
    view.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
    // 桌面/Finder 拿到焦点时也保持 active 态模糊，否则退化成扁平的 inactive 变体
    view.setState(NSVisualEffectState::Active);
    view
}

/// 窗口是透明的，方形毛玻璃会在四角露出来，所以只能靠 maskImage 变圆角。
/// 高度变了必须重画，否则圆角会被拉成椭圆。
pub fn set_mask(view: &NSVisualEffectView, width: f64, height: f64) {
    let white = NSColor::whiteColor();
    let mask = RcBlock::new(move |rect: NSRect| -> Bool {
        NSRectFillUsingOperation(rect, NSCompositingOperation::Clear);
        white.set();
        NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(rect, RADIUS, RADIUS).fill();
        Bool::YES
    });
    let image =
        NSImage::imageWithSize_flipped_drawingHandler(NSSize::new(width, height), false, &mask);
    view.setMaskImage(Some(&image));
}

/// button.primary：填充式、无描边、--accent
pub fn push_button(
    mtm: MainThreadMarker,
    title: &str,
    frame: NSRect,
    target: Option<&AnyObject>,
    action: Option<objc2::runtime::Sel>,
) -> Retained<NSButton> {
    let button = NSButton::new(mtm);
    button.setFrame(frame);
    button.setBezelStyle(NSBezelStyle::Push);
    button.setBezelColor(Some(&rgba(10.0, 132.0, 255.0, 1.0)));
    button.setTitle(&NSString::from_str(title));
    button.setFont(Some(&NSFont::systemFontOfSize(12.0)));
    wire(&button, target, action);
    button
}

/// #sites button：填充式中性的，不像 primary 那样抢强调色
pub fn text_button(mtm: MainThreadMarker, title: &str, frame: NSRect) -> Retained<NSButton> {
    let button = NSButton::new(mtm);
    button.setFrame(frame);
    button.setBezelStyle(NSBezelStyle::Push);
    button.setTitle(&NSString::from_str(title));
    button.setFont(Some(&NSFont::systemFontOfSize(12.0)));
    button
}

/// 未读点。必须 layer-backed：非 layer 视图把 alphaValue 从 0 调回 1 时 AppKit 不重绘（实测）。
pub fn set_dot(dot: &NSBox, on: bool) {
    dot.setWantsLayer(true);
    dot.setAlphaValue(if on { 1.0 } else { 0.0 });
}

/// 状态行：文案 + tooltip（data-tip 的等价物）+ 出错时换成 --warn
pub fn set_status(field: &NSTextField, text: &str, tip: &str, error: bool) {
    field.setStringValue(&NSString::from_str(text));
    field.setToolTip(Some(&NSString::from_str(tip)));
    let tint = if error {
        rgba(229.0, 161.0, 60.0, 1.0)
    } else {
        NSColor::secondaryLabelColor()
    };
    field.setTextColor(Some(&tint));
}

pub fn set_title(button: &NSButton, title: &str, tip: Option<&str>) {
    button.setTitle(&NSString::from_str(title));
    button.setToolTip(tip.map(NSString::from_str).as_deref());
}

/// .icon-btn：无边框的窗口动作键（▸ / ▾ / ✕）
pub fn glyph_button(
    mtm: MainThreadMarker,
    glyph: &str,
    frame: NSRect,
    target: Option<&AnyObject>,
    action: Option<objc2::runtime::Sel>,
) -> Retained<NSButton> {
    let button = NSButton::new(mtm);
    button.setFrame(frame);
    button.setBordered(false);
    button.setTitle(&NSString::from_str(glyph));
    button.setFont(Some(&NSFont::systemFontOfSize(12.0)));
    button.setContentTintColor(Some(&NSColor::secondaryLabelColor()));
    wire(&button, target, action);
    button
}

fn wire(button: &NSButton, target: Option<&AnyObject>, action: Option<objc2::runtime::Sel>) {
    unsafe {
        button.setTarget(target);
        button.setAction(action);
    }
}

/// 非激活悬浮面板：Borderless 时不可为 key（chip），Titled 时可 key（panel）
pub fn panel(
    mtm: MainThreadMarker,
    width: f64,
    height: f64,
    origin: NSPoint,
    style: NSWindowStyleMask,
) -> Retained<NSPanel> {
    let panel = NSPanel::initWithContentRect_styleMask_backing_defer(
        NSPanel::alloc(mtm),
        rect(origin.x, origin.y, width, height),
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
    panel.setAppearance(
        NSAppearance::appearanceNamed(unsafe { NSAppearanceNameVibrantDark }).as_deref(),
    );
    // 无边框拖动（app-region: drag）
    panel.setMovableByWindowBackground(true);
    panel.setHidesOnDeactivate(false);
    panel
}
