//! 两个窗口共用的视图构造：HUD 毛玻璃 + 圆角遮罩 + 卡片 + 文本/按钮 + 设计令牌。
//!
//! 令牌原本逐条对齐 packages/panel/static/style.css（那是 Electron 版的设计源）。
//! 这一版把「同一套层级语言」搬到 AppKit 上，同时修掉两条只在原生侧成立的偏差：
//! ① 控件的强调色不再依赖 App 是否处于激活态（见 [`Pill`]）；
//! ② 间距收敛到 4pt 网格（8 组内 / 12 行间 / 16 边距 / 25 组间），不再是 6·8·12·16 混用。
//! 每处与 CSS 不同的取值都在下面注明替换的是哪条。

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{define_class, msg_send, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::*;
use objc2_foundation::{NSObjectProtocol, NSPoint, NSRect, NSSize, NSString};
use objc2_quartz_core::{kCAMediaTimingFunctionEaseOut, CAMediaTimingFunction};

/* ---------- 圆角 ---------- */
/// 窗口圆角：毛玻璃层的 cornerRadius 与 veil 卡共用（CSS: border-radius 12）
pub const R_WINDOW: f64 = 12.0;
/// 容器：输入框、会话浏览器（CSS 给 textarea 写的也是 6，但 6 在 8 的容器体系里偏挤，
/// 提到 8 让它明确属于「容器」而不是「控件」这一档）
pub const R_FIELD: f64 = 8.0;
/// 控件：胶囊按钮、模式段（CSS: button border-radius 6）
pub const R_CTRL: f64 = 6.0;

/* ---------- 间距：4pt 网格 ---------- */
/// 光学微调档（点与文字、圆角与基线之间）
pub const S1: f64 = 4.0;
/// 组内：一行里控件之间（原 INNER=6 与 GAP=8 并存，两档差 2pt 读不出层级，合成一档）
pub const S2: f64 = 8.0;
/// 行间：正文一叠行之间的固定缝隙（CSS: #panel gap 12）
pub const S3: f64 = 12.0;
/// 边距：面板左右与底部（CSS 是 12/12；16 让 400pt 宽的内容不至于顶到圆角）
pub const S4: f64 = 16.0;

pub const PAD_TOP: f64 = S2;
pub const PAD_X: f64 = S4;
pub const PAD_BOTTOM: f64 = S4;
/// 组内间距的别名，保留名字让 chip/panel 读起来一致
pub const GAP: f64 = S2;
/// 行间间距的别名
pub const ROW_GAP: f64 = S3;
/// 组间：12 + 1px 发丝 + 12 = 25。发丝行本身高 1，见 [`divider`]
pub const SECTION_LINE: f64 = 1.0;

/* ---------- 控件尺寸 ---------- */
/// .icon-btn 22×22 → 24：▾/✕ 的可点面积原本小于视觉预期，24 与行高对齐
pub const ICON: f64 = 24.0;
pub const HEAD_H: f64 = 24.0;
/// 一行文字的高度（CSS: .line-slot 16）
pub const LINE_H: f64 = 16.0;
/// 正文控件高度（CSS: button min-height 24）
pub const CTRL_H: f64 = 24.0;
/// 未读点直径（CSS: #chip-badge 8）
pub const DOT: f64 = 8.0;
/// 胶囊按钮左右内边距（CSS: button padding 4px 12px → 取 8+8，400pt 面板里更克制）
pub const PILL_PAD_X: f64 = 10.0;

/* ---------- 字号 ---------- */
/// 正文 / 控件 / 状态行（CSS: 12px）
pub const T_BODY: f64 = 12.0;
/// 元信息与列表副行（CSS 没有这一档；11 让「组装后 N 字」这类附属事实不再和状态行抢层级）
pub const T_META: f64 = 11.0;
/// 分组标题 / 会话判定行（CSS: body 13px —— 面板里唯一用到 13 的两处，都是「先读这一行」）
pub const T_HEAD: f64 = 13.0;

/* ---------- 动效 ----------
 * 原生侧只能用 NSAnimationContext / 逐帧重画，所以曲线名落到 NSAnimationContext 的
 * 预设上：入场用「先快后慢」的 decelerate（CSS 的 --ease 是 ease-in-out，
 * 对一个按需弹出的面板来说起步太慢），退场用 accelerate。
 * 时长对照 motion-plan 的 Duration Table：卡片入场 200-350ms、微反馈 <150ms。
 */
/// 入场：淡入 + 上浮（原 ENTER_MS 0.18 / ENTER_RISE 4）
pub const MOTION_IN: f64 = 0.22;
/// 入场位移（原 4px）
pub const MOTION_RISE: f64 = 6.0;
/// 微反馈（模式滑块、高度过渡）：motion-plan 的「<150ms，不能让人觉得在等」
pub const MOTION_SNAP: f64 = 0.12;

/// NSCell.h: `NSCellHighlightByGrayPoint = 1 << 0`
const HIGHLIGHT_BY_GRAY_POINT: usize = 1 << 0;

/* ---------- 颜色 ---------- */

pub fn rect(x: f64, y: f64, w: f64, h: f64) -> NSRect {
    NSRect::new(NSPoint::new(x, y), NSSize::new(w, h))
}

pub fn rgba(r: f64, g: f64, b: f64, a: f64) -> Retained<NSColor> {
    NSColor::colorWithRed_green_blue_alpha(r / 255.0, g / 255.0, b / 255.0, a)
}

/// --bg：毛玻璃之上的那层 veil。CSS 是 .40，原生提到 .55：
/// HUD 材质在浅色桌面上会把 12pt 的 secondaryLabel 冲到对比度不足（走查 A1），
/// 加实 veil 比改文字颜色更 native —— 文字色继续用系统的 label 系列。
pub fn veil() -> Retained<NSColor> {
    rgba(20.0, 20.0, 24.0, 0.55)
}
/// --line：只用于容器与分组的发丝，控件一律不描边
pub fn hairline() -> Retained<NSColor> {
    rgba(255.0, 255.0, 255.0, 0.10)
}
/// --fill：中性控件的填充（CSS .07 在 .55 的 veil 上几乎不可见，提到 .10）
pub fn fill_ctrl() -> Retained<NSColor> {
    rgba(255.0, 255.0, 255.0, 0.10)
}
/// 轨道 / 更弱一档的底（模式开关的槽）
pub fn fill_track() -> Retained<NSColor> {
    rgba(255.0, 255.0, 255.0, 0.07)
}
/// 输入区：CSS 的 rgba(0,0,0,.28) 带一条 1px 灰边（NSScrollView LineBorder），
/// 换成圆角 8 的无边框盒，暗底略降，避免在毛玻璃上打出一块「死黑方洞」
pub fn fill_field() -> Retained<NSColor> {
    rgba(0.0, 0.0, 0.0, 0.26)
}
/// --accent：macOS 暗色系统蓝
pub fn accent() -> Retained<NSColor> {
    rgba(10.0, 132.0, 255.0, 1.0)
}
/// 列表选中行的侧栏蓝底（CSS: .br-item.sel rgba(10,132,255,.22)）
pub fn accent_tint() -> Retained<NSColor> {
    rgba(10.0, 132.0, 255.0, 0.22)
}
/// 破坏性操作（CSS: #pe-del rgba(222,60,51,.85)）
pub fn danger() -> Retained<NSColor> {
    rgba(222.0, 60.0, 51.0, 0.85)
}
/// --warn：出错与「丢了几类上下文」
pub fn warn() -> Retained<NSColor> {
    rgba(229.0, 161.0, 60.0, 1.0)
}
/// 正文主色（跟随 VibrantDark 外观，不用 CSS 的 --fg）
pub fn ink() -> Retained<NSColor> {
    NSColor::labelColor()
}

/// 列表副行 / 时间戳：比 dim 再退一档，让「一行里只有一个主角」
pub fn faint() -> Retained<NSColor> {
    NSColor::tertiaryLabelColor()
}
/// 次要色
pub fn dim() -> Retained<NSColor> {
    NSColor::secondaryLabelColor()
}
pub fn font(size: f64) -> Retained<NSFont> {
    NSFont::systemFontOfSize(size)
}

/// 动效曲线：入场用 decelerate（先快后慢，物件「被放到屏幕上」）。
/// CSS 那边统一是 --ease: cubic-bezier(.25,.1,.25,1)，即 ease-in-out；
/// 对一个按需弹出的面板来说起步太慢，所以这里换成 CoreAnimation 的命名曲线。
pub fn ease_out() -> Retained<CAMediaTimingFunction> {
    unsafe { CAMediaTimingFunction::functionWithName(kCAMediaTimingFunctionEaseOut) }
}

/* ---------- 视图工厂 ---------- */

/// 圆角矩形：veil 卡、胶囊底、输入盒、发丝线都走这里
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

/// 盒子里给子视图用的坐标面。
///
/// NSBox 会把 contentView 往里缩（实测四边各约 5pt），而全代码库都拿 `host.bounds()`
/// 给子视图定位 —— 缩进不补回来，子视图坐标系就整体下移：胶囊里居中的标题实测比盒子
/// 中心高出 5.75pt（「取入选区」顶在蓝底上半截），模式轨道的段高只剩 14pt（滑块成薄片）。
/// 返回的矩形与盒子的可见边框重合，且已经是 contentView 自己的坐标系（可负、可超出，
/// NSBox 不裁子视图 —— 实测标题就画在缩进区上方）。
pub fn face(host: &NSBox) -> NSRect {
    let outer = host.frame().size;
    match host.contentView() {
        // 用 contentView 在盒子里的真实原点，别拿「(内宽 − 外宽) / 2」去猜：
        // NSBox 的四边缩进不对称，取平均会让整组子视图一起偏出盒子中心
        // （实测模式轨道里的滑块与两段文字整体比轨道中线高约 5pt）。
        Some(content) => {
            let inner = content.frame();
            rect(-inner.origin.x, -inner.origin.y, outer.width, outer.height)
        }
        None => rect(0.0, 0.0, outer.width, outer.height),
    }
}

/// 组间发丝：CSS 的 #ctx-group/#act-group/#settings { border-top: 1px solid --line }
pub fn divider(mtm: MainThreadMarker, width: f64) -> Retained<NSBox> {
    card(
        mtm,
        rect(0.0, 0.0, width, SECTION_LINE),
        0.0,
        &hairline(),
        None,
    )
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
    field.setFont(Some(&font(size)));
    field.setTextColor(Some(tint));
    field
}

/// vibrancy:'hud' + visualEffectState:'active'
/// 窗口是透明的，方形毛玻璃会在四角露出来，所以要裁圆角。
///
/// 用图层的 cornerRadius + masksToBounds，不用 NSVisualEffectView 的 maskImage：后者是一张
/// 与窗口等大的位图，高度动画里每帧都得重画（不重画圆角会被拉成椭圆），实测每帧多花约 1ms，
/// 而且每帧新建一张 2.3MB 的图 —— 快速展开/折叠时把 phys_footprint 顶到 80MB。
/// 图层圆角与尺寸无关，高度随便变。
pub fn blur(mtm: MainThreadMarker, width: f64, height: f64) -> Retained<NSVisualEffectView> {
    let view = NSVisualEffectView::initWithFrame(
        NSVisualEffectView::alloc(mtm),
        rect(0.0, 0.0, width, height),
    );
    view.setMaterial(NSVisualEffectMaterial::HUDWindow);
    view.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
    // 桌面/Finder 拿到焦点时也保持 active 态模糊，否则退化成扁平的 inactive 变体
    view.setState(NSVisualEffectState::Active);
    view.setWantsLayer(true);
    if let Some(layer) = view.layer() {
        layer.setCornerRadius(R_WINDOW);
        layer.setMasksToBounds(true);
    }
    view
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
    let tint = if error { warn() } else { dim() };
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

/* ---------- 胶囊按钮 ---------- */

/// 一个填充式胶囊：圆角底盒 + 覆盖其上的无边框按钮。
///
/// 为什么不用 NSButton.bezelColor：系统 push bezel 在 App 非激活时会褪成灰色，
/// 而这个面板按设计永不抢前台（NonactivatingPanel + chip 不为 key），
/// 于是「取入选区 / 复制 Prompt / 选区直通」在真实使用态里和次级按钮长得一样
/// （实测：展开面板后主按钮是灰的，点中任一弹框把窗口提成 key 之后才变蓝）。
/// NSBox 的填充色不受 key/active 态影响，层级就稳住了。
///
/// 按下反馈仍走单元格的 NSCellHighlightByGrayPoint：alphaValue 与 contentTintColor
/// 在这套视图层级里都不渲染（实测），只有灰点高亮改得到像素。
#[derive(Clone)]
pub struct Pill {
    host: Retained<NSBox>,
    button: Retained<NSButton>,
}

impl Pill {
    pub fn new(
        mtm: MainThreadMarker,
        title: &str,
        frame: NSRect,
        fill: &NSColor,
        radius: f64,
    ) -> Self {
        let host = card(mtm, frame, radius, fill, None);
        let button = NSButton::new(mtm);
        button.setBordered(false);
        button.setTitle(&NSString::from_str(title));
        button.setFont(Some(&font(T_BODY)));
        button.setFrame(face(&host));
        button.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        let cell: Retained<NSButtonCell> = unsafe { msg_send![&button, cell] };
        cell.setHighlightsBy(NSCellStyleMask(HIGHLIGHT_BY_GRAY_POINT));
        // NSBox 的子视图一律进它的 contentView：边框/圆角由盒子画，按钮只管标题与命中
        if let Some(content) = host.contentView() {
            content.addSubview(&button);
        } else {
            host.addSubview(&button);
        }
        Self { host, button }
    }

    /// 排版时真正被摆放的是盒子（按钮跟着它的 contentView 走）
    pub fn view(&self) -> &NSView {
        &self.host
    }

    pub fn button(&self) -> &NSButton {
        &self.button
    }

    pub fn frame(&self) -> NSRect {
        self.host.frame()
    }

    pub fn set_frame(&self, frame: NSRect) {
        self.host.setFrame(frame);
        // 每次摆放都重算：NSBox 的 contentView 缩进要等盒子自己被布局之后才准，
        // 只在构造时算一次会拿到「还没缩」的面 —— 实测「复制 Prompt」那颗因此比盒子低 5.75pt，
        // 而头部「取入选区」正好（两者构造与摆放的时机不同）。
        self.button.setFrame(face(&self.host));
    }

    pub fn set_title(&self, title: &str) {
        set_title(&self.button, title);
    }

    pub fn set_tip(&self, tip: Option<&str>) {
        set_tip(&self.host, tip);
        set_tip(&self.button, tip);
    }

    pub fn set_fill(&self, fill: &NSColor) {
        self.host.setFillColor(fill);
    }

    /// 按标题量宽（CSS: padding 4px 12px + min-height 24）。返回胶囊应有的宽度。
    pub fn fit_width(&self, min: f64) -> f64 {
        self.button.sizeToFit();
        let text = self.button.frame().size.width;
        (text + 2.0 * PILL_PAD_X).max(min).ceil()
    }

    /// 量宽并落位到 (x, y)，高度固定 CTRL_H
    pub fn place(&self, x: f64, y: f64, min: f64) -> f64 {
        let width = self.fit_width(min);
        self.set_frame(rect(x, y, width, CTRL_H));
        width
    }
}

/// button.primary：填充式、无描边、--accent + 白字
/// 标题色不用管：窗口挂的是 NSAppearanceNameVibrantDark，labelColor 本身就是浅色，
/// 蓝底 / 红底上直接可读。（contentTintColor 对只有标题的无边框按钮不生效，实测过。）
pub fn primary_pill(mtm: MainThreadMarker, title: &str, frame: NSRect) -> Pill {
    Pill::new(mtm, title, frame, &accent(), R_CTRL)
}

/// 中性控件：层级靠明度，不靠描边（CSS: button { background: --fill }）
pub fn neutral_pill(mtm: MainThreadMarker, title: &str, frame: NSRect) -> Pill {
    Pill::new(mtm, title, frame, &fill_ctrl(), R_CTRL)
}

/// 破坏性操作：实红底 + 白字（CSS: #pe-del）
pub fn danger_pill(mtm: MainThreadMarker, title: &str, frame: NSRect) -> Pill {
    Pill::new(mtm, title, frame, &danger(), R_CTRL)
}

/// .icon-btn：无边框的动作键（▾ / ✕ / 浏览器行的整行热区）
///
/// 无边框按钮按下去在 AppKit 里默认一个像素都不变（实测 ▸ 按住前后像素差为 0），
/// 而 style.css 用 button:active{scale(.97)} 补这个反馈；这里改用单元格的
/// NSCellHighlightByGrayPoint（objc2 0.3.2 没绑定这组常量，值取自 NSCell.h）。
pub fn glyph_button(mtm: MainThreadMarker, glyph: &str, frame: NSRect) -> Retained<NSButton> {
    let button = NSButton::new(mtm);
    button.setFrame(frame);
    button.setBordered(false);
    button.setTitle(&NSString::from_str(glyph));
    button.setFont(Some(&font(T_BODY)));
    button.setContentTintColor(Some(&dim()));
    let cell: Retained<NSButtonCell> = unsafe { msg_send![&button, cell] };
    cell.setHighlightsBy(NSCellStyleMask(HIGHLIGHT_BY_GRAY_POINT));
    button
}

/// 模式轨道里的段标签：只画字与吃命中，选中底色由轨道上那颗滑块给。
///
/// 这里不能再套一层 Pill —— NSBox 会把 contentView 往里缩，盒中盒套两层，
/// 文字就从胶囊底色上探出来（实测错位约 5pt）。标签与滑块同为轨道的子孙，
/// 共用一个坐标面，才对得齐。
pub fn segment_label(mtm: MainThreadMarker, title: &str, frame: NSRect) -> Retained<NSButton> {
    let button = NSButton::new(mtm);
    button.setFrame(frame);
    button.setBordered(false);
    button.setTitle(&NSString::from_str(title));
    button.setFont(Some(&font(T_BODY)));
    button.setAlignment(NSTextAlignment::Center);
    let cell: Retained<NSButtonCell> = unsafe { msg_send![&button, cell] };
    cell.setHighlightsBy(NSCellStyleMask(HIGHLIGHT_BY_GRAY_POINT));
    button
}

/// 设置披露行（CSS: `#settings summary` 「plain text + chevron, fill only on hover」）。
///
/// 故意不铺底：这一叠行里站点、动作、保存全是实心胶囊，再给分组标题铺一层底就是把
/// 「组」和「组里的一个动作」压成同一个视觉重量。macOS 的 NSDisclosureTriangle 也是裸的。
/// 开合用换字形表示（▸ / ▾），而不是 CSS 的 transform: rotate(90deg)。
pub fn disclosure(mtm: MainThreadMarker, title: &str, frame: NSRect) -> Retained<NSButton> {
    let button = NSButton::new(mtm);
    button.setFrame(frame);
    button.setBordered(false);
    button.setTitle(&NSString::from_str(title));
    button.setFont(Some(&font(T_HEAD)));
    // NSButtonCell 默认把标题居中：72pt 宽的行里「▸ 设置」会被推到中间，
    // 左边比上面那行站点按钮缩进十几 pt，读起来像凭空多了一级缩进
    button.setAlignment(NSTextAlignment::Left);
    let cell: Retained<NSButtonCell> = unsafe { msg_send![&button, cell] };
    cell.setHighlightsBy(NSCellStyleMask(HIGHLIGHT_BY_GRAY_POINT));
    button
}

/// 带底槽的动作键：chip 的 ▸ 与面板的 ▾ 原本是裸字形，读不出「可点」
/// （走查 V3：与站点按钮相比没有任何控件暗示）。给它一档比中性控件更浅的底。
pub fn socket_button(mtm: MainThreadMarker, glyph: &str, frame: NSRect) -> Pill {
    Pill::new(mtm, glyph, frame, &fill_track(), R_CTRL)
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
    // 圆角由毛玻璃图层与 NSBox 负责，所以窗口自身不画圆角、也不画背景
    window.setOpaque(false);
    window.setBackgroundColor(Some(&NSColor::clearColor()));
    // 不开窗口投影：无边框窗口的投影按**矩形**内容轮廓算，四角外缘会留下一圈没被投影
    // 盖到的亮直角（背后是浅色窗口时特别明显）。图层 cornerRadius 只裁绘制，改不了投影
    // 形状；NSWindow.setContentShape: 在这台系统上直接抛 ObjC 异常（Rust 接不住，当场 abort）；
    // 回到从前的 maskImage 也没用 —— 拿 HEAD 那版逐像素比对过，四角同样有。
    // 关掉之后靠 1px 发丝描边 + veil 仍然分得清层次，见 docs §8。
    window.setHasShadow(false);
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
