//! 两个窗口共用的视图构造：HUD 毛玻璃 + 圆角遮罩 + 卡片 + 文本/按钮。
//! 数值全部对齐 packages/panel/static/style.css 的设计令牌。

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool, Sel};
use objc2::{define_class, msg_send, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::*;
use objc2_foundation::{NSObjectProtocol, NSPoint, NSRect, NSSize, NSString};

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
pub const LINE_H: f64 = 16.0;
pub const BADGE: f64 = 8.0;
/// NSCell.h: `NSCellHighlightByGrayPoint = 1 << 0`
const HIGHLIGHT_BY_GRAY_POINT: usize = 1 << 0;

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
/// （试过整窗复用一张、只 setSize：30 次展开/折叠的 footprint 曲线与每次重建完全重合，
///  35/71/34 vs 35/72/34，所以不必为它多养一个字段。）
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

pub fn set_title(button: &NSButton, title: &str) {
    button.setTitle(&NSString::from_str(title));
}

pub fn set_tip(view: &NSView, tip: Option<&str>) {
    view.setToolTip(tip.map(NSString::from_str).as_deref());
}

/// target/action 由 Controller 在构造之后统一挂（按钮、下拉、分段控件都是 NSControl）
pub fn wire(control: &impl AsRef<NSControl>, target: Option<&AnyObject>, action: Sel) {
    let control = control.as_ref();
    unsafe {
        control.setTarget(target);
        control.setAction(Some(action));
    }
}

/// button.primary：填充式、无描边、--accent
pub fn push_button(mtm: MainThreadMarker, title: &str, frame: NSRect) -> Retained<NSButton> {
    styled_button(mtm, title, frame, Some(&rgba(10.0, 132.0, 255.0, 1.0)))
}

/// #sites button：填充式中性的，不像 primary 那样抢强调色
pub fn text_button(mtm: MainThreadMarker, title: &str, frame: NSRect) -> Retained<NSButton> {
    styled_button(mtm, title, frame, None)
}

fn styled_button(
    mtm: MainThreadMarker,
    title: &str,
    frame: NSRect,
    bezel: Option<&NSColor>,
) -> Retained<NSButton> {
    let button = NSButton::new(mtm);
    button.setFrame(frame);
    button.setBezelStyle(NSBezelStyle::Push);
    if let Some(color) = bezel {
        button.setBezelColor(Some(color));
    }
    button.setTitle(&NSString::from_str(title));
    button.setFont(Some(&NSFont::systemFontOfSize(12.0)));
    button
}

/// .icon-btn：无边框的动作键（▸ / ▾ / ✕ / 浏览器行的整行热区）
///
/// 无边框按钮按下去在 AppKit 里默认一个像素都不变（实测 ▸ 按住前后像素差为 0），
/// 而 style.css 用 button:active{scale(.97)} 补这个反馈；这里改用单元格的
/// NSCellHighlightByGrayPoint（objc2 0.3.2 没绑定这组常量，值取自 NSCell.h）。
pub fn glyph_button(mtm: MainThreadMarker, glyph: &str, frame: NSRect) -> Retained<NSButton> {
    let button = NSButton::new(mtm);
    button.setFrame(frame);
    button.setBordered(false);
    button.setTitle(&NSString::from_str(glyph));
    button.setFont(Some(&NSFont::systemFontOfSize(12.0)));
    button.setContentTintColor(Some(&NSColor::secondaryLabelColor()));
    let cell: Retained<NSButtonCell> = unsafe { msg_send![&button, cell] };
    cell.setHighlightsBy(NSCellStyleMask(HIGHLIGHT_BY_GRAY_POINT));
    button
}

define_class! {
    /// 无边框窗口默认拿不到键盘焦点，而 panel 里的多行编辑框需要 —— 对应 Electron 的
    /// frameless + focusable:true。Titled 也能成 key，但 AppKit 会在 order-front 时
    /// constrainFrameRect，把我们算好的锚点改掉（实测 x<221 会被推到 221），所以走子类这条路。
    #[unsafe(super(NSPanel))]
    #[thread_kind = MainThreadOnly]
    pub struct KeyablePanel;

    impl KeyablePanel {
        #[unsafe(method(canBecomeKeyWindow))]
        fn can_become_key(&self) -> bool {
            true
        }

        /// renderer.js 里 document mousedown 命中 INPUT/TEXTAREA/SELECT 才调 focusSelf()：
        /// 只有点到能输入的东西才把窗口提成 key，其余交互一律不抢前台焦点。
        #[unsafe(method(sendEvent:))]
        fn send_event(&self, event: &NSEvent) {
            if event.r#type() == NSEventType::LeftMouseDown {
                let point = event.locationInWindow();
                let editable = self
                    .contentView()
                    .and_then(|root| root.hitTest(point))
                    .map(|hit| {
                        hit.isKindOfClass(objc2::class!(NSTextView))
                            || hit.isKindOfClass(objc2::class!(NSPopUpButton))
                    })
                    .unwrap_or(false);
                if editable && !self.isKeyWindow() {
                    self.makeKeyAndOrderFront(None);
                }
            }
            unsafe { msg_send![super(self), sendEvent: event] }
        }
    }

    unsafe impl NSObjectProtocol for KeyablePanel {}
}

fn configure(window: &NSPanel) {
    unsafe {
        window.setReleasedWhenClosed(false);
    }
    // 圆角由 maskImage 与 NSBox 负责，所以窗口自身不画圆角、也不画背景
    window.setOpaque(false);
    window.setBackgroundColor(Some(&NSColor::clearColor()));
    window.setHasShadow(true);
    window.setLevel(NSFloatingWindowLevel);
    // 等价于 alwaysOnTop + setVisibleOnAllWorkspaces({ visibleOnFullScreen: true })
    window.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::FullScreenAuxiliary,
    );
    window.setAppearance(
        NSAppearance::appearanceNamed(unsafe { NSAppearanceNameVibrantDark }).as_deref(),
    );
    // 无边框拖动（app-region: drag）
    window.setMovableByWindowBackground(true);
    window.setHidesOnDeactivate(false);
}

/// chip：Borderless 的 NSPanel，不能成为 key，所以永不抢键盘焦点
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
    configure(&panel);
    panel
}

/// panel：同样无边框，但允许成为 key window（设置里的编辑框要用键盘）
pub fn keyable_panel(
    mtm: MainThreadMarker,
    width: f64,
    height: f64,
    origin: NSPoint,
) -> Retained<NSPanel> {
    let frame = rect(origin.x, origin.y, width, height);
    let style = NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel;
    let panel: Retained<KeyablePanel> = unsafe {
        msg_send![
            KeyablePanel::alloc(mtm),
            initWithContentRect: frame,
            styleMask: style,
            backing: NSBackingStoreType::Buffered,
            defer: false,
        ]
    };
    configure(&panel);
    panel.into_super()
}
