//! panel 窗口：展开态。头部固定，正文是一叠「行」—— 模式与折叠状态决定哪些行在场，
//! 面板高度由可见行加起来（超过工作区就滚动），所以从不写死。
//! 正文按三个组排：上下文 / 输出 / 设置，组间一条发丝（CSS: #ctx-group/#act-group/#settings
//! 的 border-top），组内 8、行间 12、组间 12+1+12。
//! 对照 static/index.html 的 #panel 结构与 renderer.js 的 applyMode()。

use std::cell::{Cell, RefCell};
use std::ptr::NonNull;
use std::rc::Rc;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{msg_send, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAnimationContext, NSBorderType, NSBox, NSButton, NSColor, NSControlStateValueOff,
    NSControlStateValueOn, NSPanel, NSPopUpButton, NSScrollView, NSSwitch, NSTextAlignment,
    NSTextField, NSTextView, NSView, NSVisualEffectView, NSWorkspace,
};
use objc2_foundation::{NSArray, NSPoint, NSRect, NSSize, NSString};

use crate::ctxpack::adapters::SessionRef;
use crate::flipped::{flipped_clip, flipped_view};
use crate::geo::{Geometry, EDGE};
use crate::settings::{PromptTemplate, SessionPath, SiteTarget};
use crate::views::Pill;
use crate::views::{
    self, CTRL_H, DOT, GAP, HEAD_H, ICON, LINE_H, PAD_BOTTOM, PAD_TOP, PAD_X, ROW_GAP, R_CTRL,
    R_FIELD, R_WINDOW, S1, S2, S3, T_BODY, T_HEAD, T_META,
};

pub const WIDTH: f64 = 400.0;
/// Electron 的 autoHeight 下限
/// 建窗时的占位高度，第一次 arrange 之后正文就多高算多高
const INIT_BODY: f64 = 240.0;
/// 应急下限：约四行
const MIN_ROOM: f64 = 96.0;
/// 组内行距（CSS: #ctx-group / #act-group gap 8），与组间的 ROW_GAP 12 成对
const GROUP_GAP: f64 = GAP;
/// 多行编辑框（对应 textarea rows=3~5）
const FIELD: f64 = 66.0;
const SESSIONS_FIELD: f64 = 92.0;
const BROWSER_LINE: f64 = 36.0;
const BROWSER_MAX: f64 = 168.0;
/// 会话列表一行里三块文字的宽度（名称吃掉剩下的）
const BROWSER_LEFT: f64 = 118.0;
const BROWSER_TIME: f64 = 62.0;
const BODY_W: f64 = WIDTH - 2.0 * PAD_X;
/// 模式开关：两段各 4 个汉字，宽度固定，切模式时右侧的下拉不会跟着挪
const MODE_W: f64 = 148.0;
const AGENT_W: f64 = 112.0;
/// 模式轨道的内边距与每段宽度（CSS: #mode-seg { padding:2; gap:2 }）
const SEG_INSET: f64 = 2.0;
const SEG_W: f64 = (MODE_W - 3.0 * SEG_INSET) / 2.0;

/// 正文里的行。`on` 是 CSS 的 display:none（不占位）；模式内的控件用 setHidden（占位不画），
/// 两种语义分开，切换时才不会跳版。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Row {
    DivCtx,
    Mode,
    CtxActions,
    Session,
    CtxStatus,
    Browser,
    DivAct,
    Actions,
    PackMeta,
    DivSet,
    Settings,
    Editor,
}

struct RowSpec {
    kind: Row,
    view: Retained<NSView>,
    height: Cell<f64>,
    on: Cell<bool>,
}

pub struct Panel {
    pub window: Retained<NSPanel>,
    blur: Retained<NSVisualEffectView>,
    card: Retained<NSBox>,
    // ---- 头部 ----
    pub collapse: Retained<NSButton>,
    pub status: Retained<NSTextField>,
    pub capture: Pill,
    pub quit: Retained<NSButton>,
    pub unread: Retained<NSBox>,
    // ---- 正文 ----
    scroll: Retained<NSScrollView>,
    doc: Retained<NSView>,
    rows: Vec<RowSpec>,
    /// 模式开关：轨道 + 一颗滑块 + 两个纯标签。标签直接挂在轨道的内容面上 ——
    /// 再套一层胶囊盒就会被 NSBox 各缩一层内边距，文字从滑块上探出去（实测约 5pt）。
    mode_track: Retained<NSBox>,
    mode_thumb: Retained<NSBox>,
    pub mode_read: Retained<NSButton>,
    pub mode_direct: Retained<NSButton>,
    /// 当前显示的是哪个模式，用来判断滑块该不该动（启动时不该看到它飞过来）
    mode_shown: Cell<bool>,
    selects: Retained<NSView>,
    pub agent_pick: Retained<NSPopUpButton>,
    pub turns_pick: Retained<NSPopUpButton>,
    pub browse: Pill,
    pub refresh: Pill,
    pub session_line: Retained<NSTextField>,
    pub ctx_status: Retained<NSTextField>,
    browser_box: Retained<NSBox>,
    pub browser_doc: Retained<NSView>,
    pub prompt_pick: Retained<NSPopUpButton>,
    pub copy: Pill,
    pub sites_row: Retained<NSView>,
    pub pack_meta: Retained<NSTextField>,
    pub settings_toggle: Retained<NSButton>,
    pub editor_ctx: Retained<NSView>,
    pe_bar: Retained<NSView>,
    pub pe_pick: Retained<NSPopUpButton>,
    pub pe_new: Pill,
    pub pe_del: Pill,
    pub pe_count: Retained<NSTextField>,
    pub pe_tpl: Retained<NSTextView>,
    pub set_sessions: Retained<NSTextView>,
    pub set_redact: Retained<NSSwitch>,
    redact_label: Retained<NSTextField>,
    pub sites_label: Retained<NSTextField>,
    sites_box: Retained<NSBox>,
    pub set_sites: Retained<NSTextView>,
    pub set_save: Pill,
    sites: RefCell<Vec<Pill>>,
    /// 动画进行中（入场、退场、长高变矮都算）：这几帧内的 windowDidMove 不算用户挪窗口。
    /// Rc 是因为完成回调要在动画结束后把它落回 false
    animating: Rc<Cell<bool>>,
}

/// 正文高度 = 可见行高之和 + 行间距（Electron 那边量 body.scrollHeight 的等价物）
fn stack_height(rows: &[(Row, f64)]) -> f64 {
    let mut total = 0.0;
    for (index, (kind, height)) in rows.iter().enumerate() {
        if index > 0 {
            total += row_gap(rows[index - 1].0, *kind);
        }
        total += height;
    }
    total
}

/// 发丝行标的是分组边界（CSS: #ctx-group / #act-group / #settings 的 border-top）
fn is_divider(kind: Row) -> bool {
    matches!(kind, Row::DivCtx | Row::DivAct | Row::DivSet)
}

/// 组内 8pt、组间 12pt（CSS: 组内 gap 8 / #panel gap 12）。
/// 以前一律 12，两组之间和一组之内读不出层级，空着的状态行也被 12 放大成一个洞
fn row_gap(above: Row, below: Row) -> f64 {
    if is_divider(above) || is_divider(below) {
        ROW_GAP
    } else {
        GROUP_GAP
    }
}

/// 正文只有上界没有下界：Electron 的 autoHeight 量的就是内容（实测直通模式 185pt 内容
/// 被 240 的下限撑出一条 60pt 的底部空白）。切模式时高度是变的，但那是一段高度动画，
/// 不是留白。`room` 夹上界；MIN_ROOM 只是拖到屏幕最下缘时不让正文被夹成 0 的应急值。
fn body_height(wanted: f64, room: f64) -> f64 {
    wanted.min(room.max(MIN_ROOM))
}

/// 行容器一律 flipped：行内的 y 表示「离行顶多远」，换行时第二行才不会被画到第一行上面
fn container(mtm: MainThreadMarker, height: f64) -> Retained<NSView> {
    flipped_view(mtm, views::rect(0.0, 0.0, BODY_W, height)).into_super()
}

fn label(mtm: MainThreadMarker, title: &str, width: f64) -> Retained<NSTextField> {
    views::label(
        mtm,
        title,
        T_BODY,
        &views::dim(),
        views::rect(0.0, 0.0, width, LINE_H),
    )
}

fn popup(
    mtm: MainThreadMarker,
    width: f64,
    titles: &[&str],
    tip: Option<&str>,
) -> Retained<NSPopUpButton> {
    let pick = NSPopUpButton::initWithFrame_pullsDown(
        NSPopUpButton::alloc(mtm),
        views::rect(0.0, 0.0, width, CTRL_H),
        false,
    );
    let owned = titles
        .iter()
        .map(|t| NSString::from_str(t))
        .collect::<Vec<_>>();
    let refs: Vec<&NSString> = owned.iter().map(|s| &**s).collect();
    pick.addItemsWithTitles(&NSArray::from_slice(&refs));
    views::set_tip(&pick, tip);
    pick
}

/// 子视图跟着盒子一起长：NSBox 的排版面是它的 contentView
fn both_sizable() -> objc2_app_kit::NSAutoresizingMaskOptions {
    objc2_app_kit::NSAutoresizingMaskOptions::ViewWidthSizable
        | objc2_app_kit::NSAutoresizingMaskOptions::ViewHeightSizable
}

/// 子视图跟着盒子一起长：NSBox 的排版面是它的 contentView
/// 让盒子里的内容按它自己的 contentView 重算一次尺寸。
/// NSBox 的内容缩进要等盒子排过布局才是真值，只在构造时量会拿到「还没缩」的面 ——
/// 胶囊那边改成每次摆放重算（见 views::Pill::set_frame），输入框只在创建时算过一次，
/// 所以在编辑器排完版之后统一补一次。
fn refit_box(host: &NSBox) {
    let Some(content) = host.contentView() else {
        return;
    };
    let bounds = content.bounds();
    for inner in content.subviews().iter() {
        if let Some(view) = inner.downcast_ref::<NSView>() {
            view.setFrame(bounds);
        }
    }
}

fn host_box_add(host: &NSBox, view: &NSView) {
    view.setAutoresizingMask(both_sizable());
    match host.contentView() {
        Some(content) => content.addSubview(view),
        None => host.addSubview(view),
    }
}

/// 多行编辑框：圆角暗盒 + 无边框滚动区。
/// 原来直接给 NSScrollView 挂 LineBorder，是这一屏里唯一带直角硬边的东西，
/// 与 12pt 圆角的毛玻璃完全不是一族（走查 C3）。盒子画圆角与底色，滚动区只留文字。
fn text_field(
    mtm: MainThreadMarker,
    height: f64,
    tip: Option<&str>,
) -> (Retained<NSBox>, Retained<NSTextView>) {
    let host = views::card(
        mtm,
        views::rect(0.0, 0.0, BODY_W, height),
        R_FIELD,
        &views::fill_field(),
        None,
    );
    views::set_tip(&host, tip);
    let scroll = NSScrollView::new(mtm);
    scroll.setFrame(views::face(&host));
    scroll.setBorderType(NSBorderType::NoBorder);
    scroll.setHasVerticalScroller(true);
    scroll.setAutohidesScrollers(true);
    scroll.setDrawsBackground(false);
    let text = NSTextView::new(mtm);
    text.setEditable(true);
    text.setRichText(false);
    text.setBackgroundColor(&NSColor::clearColor());
    text.setFont(Some(&views::font(T_BODY)));
    text.setTextColor(Some(&views::ink()));
    text.setMinSize(NSSize::new(0.0, 0.0));
    text.setMaxSize(NSSize::new(f64::MAX, f64::MAX));
    // 内边距交给 textContainerInset，盒子的圆角才不会被首行文字顶到
    text.setTextContainerInset(NSSize::new(S3, S1 + 2.0));
    scroll.setDocumentView(Some(&text));
    host_box_add(&host, &scroll);
    (host, text)
}

/// 从左到右摆一排，放不下就换行；返回用掉的总高度（行高取该行最高的那个，站点条会比一行高）
fn flow(views_: &[&NSView], avail: f64) -> f64 {
    let mut x = 0.0;
    let mut y = 0.0;
    let mut line = CTRL_H;
    for view in views_ {
        let width = view.frame().size.width;
        if x > 0.0 && x + width > avail {
            x = 0.0;
            y += line + S2;
            line = CTRL_H;
        }
        view.setFrameOrigin(NSPoint::new(x, y));
        line = line.max(view.frame().size.height);
        x += width + S2;
    }
    y + line
}

/// 同一条发丝线，三个组各用一行（CSS 的 border-top）
fn section(mtm: MainThreadMarker) -> Retained<NSView> {
    let row = container(mtm, views::SECTION_LINE);
    row.addSubview(&views::divider(mtm, BODY_W));
    row
}

/// 「m 分钟前」这种相对时间：400pt 宽的一行放不下 toLocaleString 的完整串，
/// Electron 那版是 `.mt` 直接摆整串（走查 5.4：极端数据态破坏布局），这里压成短格式，
/// 完整时间进 tooltip。
fn short_time(mtime_ms: f64) -> String {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64() * 1000.0);
    let mins = ((now_ms - mtime_ms) / 60_000.0).floor();
    if mins < 1.0 {
        "刚刚".to_string()
    } else if mins < 60.0 {
        format!("{mins:.0} 分")
    } else if mins < 60.0 * 24.0 {
        format!("{:.0} 时", mins / 60.0)
    } else if mins < 60.0 * 24.0 * 7.0 {
        format!("{:.0} 天", mins / (60.0 * 24.0))
    } else {
        format!("{:.0} 周", mins / (60.0 * 24.0 * 7.0))
    }
}

/// 会话行的 tooltip：文件路径 + 完整时间，鼠标停上去才看得到
fn row_tip(reference: &SessionRef) -> String {
    match reference.preview.as_deref() {
        Some(preview) if !preview.is_empty() => {
            format!("{}\n{}", reference.file_path, preview)
        }
        _ => reference.file_path.clone(),
    }
}

impl Panel {
    #[allow(clippy::too_many_arguments)]
    pub fn create(
        mtm: MainThreadMarker,
        geometry: &Geometry,
        anchor: (f64, f64),
        sites: &[SiteTarget],
        prompts: &[PromptTemplate],
        session_paths: &[SessionPath],
        redact: bool,
        turns: usize,
        read_mode: bool,
        active_prompt: usize,
    ) -> Self {
        let height = INIT_BODY + PAD_TOP + HEAD_H + ROW_GAP + PAD_BOTTOM;
        let window = views::keyable_panel(
            mtm,
            WIDTH,
            height,
            geometry.cocoa_origin(anchor.0, anchor.1, height),
        );

        let blur = views::blur(mtm, WIDTH, height);
        window.setContentView(Some(&blur));
        let card = views::card(
            mtm,
            views::rect(0.0, 0.0, WIDTH, height),
            R_WINDOW,
            &views::veil(),
            Some(&views::hairline()),
        );
        blur.addSubview(&card);

        // ---------- 头部行 ----------
        // 头部左右两端都是裸符号按钮：右边 × 是，左边 ▾ 也得是。
        // 之前用 socket_button（带底座的胶囊），它看起来像一个没内容的下拉框，
        // 而不是「折回小条」——同一个头里两种 affordance 会读错。
        let collapse = views::glyph_button(mtm, "\u{25BE}", views::rect(0.0, 0.0, ICON, ICON));
        views::set_tip(&collapse, Some("折回小条"));
        let status = views::label(
            mtm,
            crate::capture::NO_SELECTION,
            T_BODY,
            &views::dim(),
            views::rect(0.0, 0.0, 1.0, LINE_H),
        );
        let capture = views::primary_pill(mtm, "取入选区", views::rect(0.0, 0.0, 74.0, CTRL_H));
        capture.set_tip(Some("把刚才复制的内容取进来"));
        let quit = views::glyph_button(mtm, "\u{2715}", views::rect(0.0, 0.0, ICON, ICON));
        views::set_tip(&quit, Some("退出常驻"));
        let unread = views::card(
            mtm,
            views::rect(0.0, 0.0, DOT, DOT),
            DOT / 2.0,
            &views::accent(),
            None,
        );
        views::set_dot(&unread, false);
        let head_views: [&NSView; 5] = [&status, &collapse, capture.view(), &quit, &unread];
        for view in head_views {
            blur.addSubview(view);
        }

        // ---------- 正文滚动区 ----------
        let doc = flipped_view(mtm, views::rect(0.0, 0.0, BODY_W, INIT_BODY));
        let scroll = NSScrollView::new(mtm);
        // 视口也得是 flipped 的：否则正文比视口矮时整叠行被按底边对齐，模式行与动作行之间
        // 凭空多出一块空洞（实测直通模式 182pt 正文 / 240pt 视口 → 58pt 空洞）
        scroll.setContentView(&flipped_clip(mtm, views::rect(0.0, 0.0, BODY_W, INIT_BODY)));
        scroll.setBorderType(NSBorderType::NoBorder);
        scroll.setHasVerticalScroller(true);
        scroll.setAutohidesScrollers(true);
        scroll.setDrawsBackground(false);
        scroll.setDocumentView(Some(&doc));
        blur.addSubview(&scroll);

        // 1 模式行：自绘的「轨道 + 两段」，不用 NSSegmentedControl ——
        // 系统的选中段（含 setSelectedSegmentBezelColor）只在 App 激活时才上色，
        // 而本面板按设计永不激活，于是「我在哪个模式」这个最重要的信息在真实使用态里读不出来。
        // 轨道与段都是 NSBox/胶囊，颜色与 key / active 态无关。
        let mode_track = views::card(
            mtm,
            views::rect(0.0, 0.0, MODE_W, CTRL_H),
            R_CTRL + SEG_INSET,
            &views::fill_track(),
            None,
        );
        let mode_thumb = views::card(
            mtm,
            views::rect(SEG_INSET, SEG_INSET, SEG_W, CTRL_H - 2.0 * SEG_INSET),
            R_CTRL,
            &views::accent(),
            None,
        );
        let segment = |title: &str, tag: isize, tip: &str| {
            let x = SEG_INSET + (tag as f64) * (SEG_W + SEG_INSET);
            let label = views::segment_label(
                mtm,
                title,
                views::rect(x, SEG_INSET, SEG_W, CTRL_H - 2.0 * SEG_INSET),
            );
            label.setTag(tag);
            views::set_tip(&label, Some(tip));
            label
        };
        let mode_read = segment("会话解读", 0, "带会话历史组装 Prompt");
        let mode_direct = segment("选区直通", 1, "逐字节原文，翻译 / 检索即用");
        // 滑块在下、标签在上：同一个坐标面，两者才对得齐
        if let Some(face) = mode_track.contentView() {
            face.addSubview(&mode_thumb);
            face.addSubview(&mode_read);
            face.addSubview(&mode_direct);
        }
        let selects = container(mtm, CTRL_H);
        let agent_pick = popup(
            mtm,
            112.0,
            &["自动判定", "Claude Code", "Codex", "WorkBuddy", "Qoder"],
            None,
        );
        let turns_pick = popup(
            mtm,
            76.0,
            &["4 轮", "8 轮", "16 轮", "30 轮", "全部"],
            Some("按轮数截取，不做字符截断"),
        );
        selects.addSubview(&agent_pick);
        selects.addSubview(&turns_pick);
        let mode_row = container(mtm, CTRL_H);
        mode_row.addSubview(&mode_track);
        mode_row.addSubview(&selects);

        // 2 上下文动作行
        let browse = views::neutral_pill(mtm, "浏览会话", views::rect(0.0, 0.0, 74.0, CTRL_H));
        browse.set_tip(Some("点一条即用它填充"));
        let refresh = views::neutral_pill(mtm, "刷新上下文", views::rect(0.0, 0.0, 74.0, CTRL_H));
        let ctx_actions = container(mtm, CTRL_H);
        ctx_actions.addSubview(browse.view());
        ctx_actions.addSubview(refresh.view());

        // 3/4 判定会话 + 状态：判定结果是这一屏最该先读到的事实，抬到 13pt 主色，
        // 与下面的 12pt 状态行、11pt 字数行形成三级层级（原来三行都是 12pt 灰字）
        let session_line = views::label(
            mtm,
            crate::capture::CONTEXT_EMPTY,
            T_HEAD,
            &views::ink(),
            views::rect(0.0, 0.0, BODY_W, LINE_H + 2.0),
        );
        let ctx_status = label(mtm, "", BODY_W);
        let session_row = container(mtm, LINE_H + 2.0);
        session_row.addSubview(&session_line);
        let status_row = container(mtm, LINE_H);
        status_row.addSubview(&ctx_status);

        // 5 会话浏览器：圆角盒 + 无边框滚动区，和输入框同一族
        let browser_doc = flipped_view(mtm, views::rect(0.0, 0.0, BODY_W - 2.0, BROWSER_LINE));
        let browser_scroll = NSScrollView::new(mtm);
        browser_scroll.setFrame(views::rect(0.0, 0.0, BODY_W, BROWSER_LINE));
        browser_scroll.setAutoresizingMask(both_sizable());
        browser_scroll.setBorderType(NSBorderType::NoBorder);
        browser_scroll.setHasVerticalScroller(true);
        browser_scroll.setAutohidesScrollers(true);
        browser_scroll.setDrawsBackground(false);
        browser_scroll.setDocumentView(Some(&browser_doc));
        let browser_box = views::card(
            mtm,
            views::rect(0.0, 0.0, BODY_W, BROWSER_LINE),
            R_FIELD,
            &views::fill_field(),
            None,
        );
        host_box_add(&browser_box, &browser_scroll);
        let browser_row = container(mtm, BROWSER_LINE);
        browser_row.addSubview(&browser_box);

        // 6 动作行
        let prompt_pick = popup(mtm, 96.0, &[], Some("增删改在设置里"));
        let copy = views::primary_pill(mtm, "复制 Prompt", views::rect(0.0, 0.0, 90.0, CTRL_H));
        copy.set_tip(Some("指令 + 选区 + 历史，一次复制"));
        let sites_row = container(mtm, CTRL_H);
        let actions = container(mtm, CTRL_H);
        actions.addSubview(&prompt_pick);
        actions.addSubview(copy.view());
        actions.addSubview(&sites_row);

        // 7 字数行：附属事实，降到 11pt，不再和状态行抢层级
        let pack_meta = views::label(
            mtm,
            "",
            T_META,
            &views::dim(),
            views::rect(0.0, 0.0, BODY_W, LINE_H),
        );
        let meta_row = container(mtm, LINE_H);
        meta_row.addSubview(&pack_meta);

        // 8 设置折叠：裸字 + 三角的披露行（CSS: #settings summary），不铺底
        let settings_toggle =
            views::disclosure(mtm, "\u{25B8}  设置", views::rect(0.0, 0.0, 72.0, CTRL_H));
        let settings_row = container(mtm, CTRL_H);
        settings_row.addSubview(&settings_toggle);

        // 9 设置编辑器
        let editor = flipped_view(mtm, views::rect(0.0, 0.0, BODY_W, FIELD));
        let editor_ctx = container(mtm, 0.0);
        let pe_pick = popup(mtm, 120.0, &[], None);
        let pe_new = views::neutral_pill(mtm, "+ 新建", views::rect(0.0, 0.0, 52.0, CTRL_H));
        let pe_del = views::danger_pill(mtm, "删除", views::rect(0.0, 0.0, 44.0, CTRL_H));
        let pe_count = views::label(
            mtm,
            "",
            T_META,
            &views::dim(),
            views::rect(0.0, 0.0, 48.0, LINE_H),
        );
        pe_count.setAlignment(NSTextAlignment::Right);
        let pe_bar = container(mtm, CTRL_H);
        pe_bar.addSubview(&pe_pick);
        pe_bar.addSubview(pe_new.view());
        pe_bar.addSubview(pe_del.view());
        pe_bar.addSubview(&pe_count);
        let (pe_tpl_box, pe_tpl) = text_field(mtm, FIELD, None);
        let sessions_label = label(mtm, "会话路径", BODY_W);
        let (sessions_box, set_sessions) = text_field(
            mtm,
            SESSIONS_FIELD,
            Some("每行：agent|路径；发现只读这里，删一行即停扫该源"),
        );
        let redact_row = container(mtm, CTRL_H);
        let set_redact = NSSwitch::new(mtm);
        set_redact.sizeToFit();
        set_redact.setFrameOrigin(NSPoint::new(
            0.0,
            (CTRL_H - set_redact.frame().size.height) / 2.0,
        ));
        set_redact.setState(if redact {
            NSControlStateValueOn
        } else {
            NSControlStateValueOff
        });
        set_redact.setToolTip(Some(&NSString::from_str("组装时家目录写作 ~")));
        let redact_label = label(mtm, "路径脱敏", 120.0);
        redact_row.addSubview(&set_redact);
        redact_row.addSubview(&redact_label);
        for view in [
            &label(mtm, "提问指令", BODY_W) as &NSView,
            &pe_bar,
            &pe_tpl_box,
            &sessions_label,
            &sessions_box,
            &redact_row,
        ] {
            editor_ctx.addSubview(view);
        }

        let sites_label = label(
            mtm,
            if read_mode {
                "会话解读目标站"
            } else {
                "直通目标站"
            },
            BODY_W,
        );
        let (sites_box, set_sites) = text_field(mtm, FIELD, Some("每行：名称|URL"));
        let set_save = views::primary_pill(mtm, "保存设置", views::rect(0.0, 0.0, 72.0, CTRL_H));
        for view in [
            &editor_ctx as &NSView,
            &sites_label,
            &sites_box,
            set_save.view(),
        ] {
            editor.addSubview(view);
        }

        let rows = vec![
            RowSpec {
                kind: Row::DivCtx,
                view: section(mtm),
                height: Cell::new(views::SECTION_LINE),
                on: Cell::new(true),
            },
            RowSpec {
                kind: Row::Mode,
                view: mode_row,
                height: Cell::new(CTRL_H),
                on: Cell::new(true),
            },
            RowSpec {
                kind: Row::CtxActions,
                view: ctx_actions,
                height: Cell::new(CTRL_H),
                on: Cell::new(read_mode),
            },
            RowSpec {
                kind: Row::Session,
                view: session_row,
                height: Cell::new(LINE_H + 2.0),
                on: Cell::new(read_mode),
            },
            RowSpec {
                kind: Row::CtxStatus,
                view: status_row,
                height: Cell::new(LINE_H),
                on: Cell::new(true),
            },
            RowSpec {
                kind: Row::Browser,
                view: browser_row,
                height: Cell::new(0.0),
                on: Cell::new(false),
            },
            RowSpec {
                kind: Row::DivAct,
                view: section(mtm),
                height: Cell::new(views::SECTION_LINE),
                on: Cell::new(true),
            },
            RowSpec {
                kind: Row::Actions,
                view: actions,
                height: Cell::new(CTRL_H),
                on: Cell::new(true),
            },
            RowSpec {
                kind: Row::PackMeta,
                view: meta_row,
                height: Cell::new(LINE_H),
                on: Cell::new(true),
            },
            RowSpec {
                kind: Row::DivSet,
                view: section(mtm),
                height: Cell::new(views::SECTION_LINE),
                on: Cell::new(true),
            },
            RowSpec {
                kind: Row::Settings,
                view: settings_row,
                height: Cell::new(CTRL_H),
                on: Cell::new(true),
            },
            RowSpec {
                kind: Row::Editor,
                view: editor.clone().into_super(),
                height: Cell::new(0.0),
                on: Cell::new(false),
            },
        ];
        // 行的父视图是 doc（滚动区的 contentView 之下），layout_rows 只管上下位置。
        // 关着的行必须一上来就 setHidden：它还没被排过版，frame 停在创建位置，
        // 不藏的话就是一块盖在上面吃点击的透明板子（实测盖住模式行，分段控件点不动）。
        for spec in &rows {
            spec.view.setHidden(!spec.on.get());
            doc.addSubview(&spec.view);
        }

        let panel = Self {
            window,
            blur,
            card,
            collapse,
            status,
            capture,
            quit,
            unread,
            scroll,
            doc: doc.clone().into_super(),
            rows,
            mode_track,
            mode_thumb,
            mode_read,
            mode_direct,
            mode_shown: Cell::new(read_mode),
            selects,
            agent_pick,
            turns_pick,
            browse,
            refresh,
            session_line,
            ctx_status,
            browser_box,
            browser_doc: browser_doc.clone().into_super(),
            prompt_pick,
            copy,
            sites_row,
            pack_meta,
            settings_toggle,
            editor_ctx,
            pe_bar: pe_bar.clone(),
            pe_pick,
            pe_new,
            pe_del,
            pe_count,
            pe_tpl,
            set_sessions,
            set_redact,
            redact_label,
            sites_label,
            sites_box,
            set_sites,
            set_save,
            sites: RefCell::new(Vec::new()),
            animating: Rc::new(Cell::new(false)),
        };
        panel.reload_sites(mtm, sites);
        // 此刻草稿就是已保存的那份，两个选择器同源
        panel.reload_prompts(prompts, active_prompt);
        panel.reload_editor_prompts(prompts, active_prompt);
        panel.set_turns(turns);
        panel.reload_session_paths(session_paths);
        panel.arrange(geometry, anchor, INIT_BODY);
        panel
    }

    fn row(&self, kind: Row) -> &RowSpec {
        self.rows.iter().find(|r| r.kind == kind).expect("已知的行")
    }

    pub fn set_row(&self, kind: Row, on: bool) {
        let spec = self.row(kind);
        spec.on.set(on);
        spec.view.setHidden(!on);
    }

    pub fn row_on(&self, kind: Row) -> bool {
        self.row(kind).on.get()
    }

    /// 直通模式：上下文区整块撤掉（display:none），但模式行内的下拉占位隐藏，切换不跳版
    pub fn set_read_mode(&self, read: bool) {
        self.paint_mode(read);
        self.selects.setHidden(!read);
        self.prompt_pick.setHidden(!read);
        self.editor_ctx.setHidden(!read);
        self.set_row(Row::CtxActions, read);
        self.set_row(Row::Session, read);
        if !read {
            self.set_row(Row::Browser, false);
        }
        self.sites_label
            .setStringValue(&NSString::from_str(if read {
                "会话解读目标站"
            } else {
                "直通目标站"
            }));
        self.copy
            .set_tip(read.then_some("指令 + 选区 + 历史，一次复制"));
    }

    pub fn agent_token(&self) -> String {
        ["auto", "claude-code", "codex", "workbuddy", "qoder"]
            .get(self.agent_pick.indexOfSelectedItem().max(0) as usize)
            .copied()
            .unwrap_or("auto")
            .to_string()
    }

    pub fn set_agent_token(&self, token: &str) {
        let index = match token {
            "claude-code" => 1,
            "codex" => 2,
            "workbuddy" => 3,
            "qoder" => 4,
            _ => 0,
        };
        self.agent_pick.selectItemAtIndex(index);
    }

    /// 轮数是唯一的裁剪旋钮：「全部」= 0
    pub fn turns(&self) -> usize {
        [4usize, 8, 16, 30, 0]
            .get(self.turns_pick.indexOfSelectedItem().max(0) as usize)
            .copied()
            .unwrap_or(8)
    }

    pub fn set_turns(&self, turns: usize) {
        let index = match turns {
            4 => 0,
            16 => 2,
            30 => 3,
            0 => 4,
            _ => 1,
        };
        self.turns_pick.selectItemAtIndex(index);
    }

    /// 主视图的指令选择器：只跟已经保存进设置的那份
    pub fn reload_prompts(&self, prompts: &[PromptTemplate], active: usize) {
        self.set_titles(&self.prompt_pick, prompts, active);
    }

    /// 编辑器里的指令选择器：跟草稿走（没保存的新建/删除也在里面）
    pub fn reload_editor_prompts(&self, prompts: &[PromptTemplate], active: usize) {
        self.set_titles(&self.pe_pick, prompts, active);
    }

    fn set_titles(&self, pick: &NSPopUpButton, prompts: &[PromptTemplate], active: usize) {
        let owned = prompts
            .iter()
            .map(|p| NSString::from_str(&p.name))
            .collect::<Vec<_>>();
        let refs: Vec<&NSString> = owned.iter().map(|s| &**s).collect();
        pick.removeAllItems();
        pick.addItemsWithTitles(&NSArray::from_slice(&refs));
        pick.selectItemAtIndex(active.min(prompts.len().saturating_sub(1)) as isize);
    }

    pub fn active_prompt(&self) -> usize {
        self.prompt_pick.indexOfSelectedItem().max(0) as usize
    }

    pub fn reload_session_paths(&self, paths: &[SessionPath]) {
        let text = paths
            .iter()
            .map(|p| format!("{}|{}", p.agent, p.path))
            .collect::<Vec<_>>()
            .join("\n");
        self.set_sessions.setString(&NSString::from_str(&text));
    }

    pub fn session_paths_text(&self) -> String {
        self.set_sessions.string().to_string()
    }

    pub fn template_text(&self) -> String {
        self.pe_tpl.string().to_string()
    }

    pub fn set_template_text(&self, text: &str) {
        self.pe_tpl.setString(&NSString::from_str(text));
    }

    pub fn sites_text(&self) -> String {
        self.set_sites.string().to_string()
    }

    pub fn set_sites_text(&self, text: &str) {
        self.set_sites.setString(&NSString::from_str(text));
    }

    pub fn redact_on(&self) -> bool {
        self.set_redact.state() == NSControlStateValueOn
    }

    pub fn prompt_count(&self, index: usize, total: usize) {
        self.pe_count
            .setStringValue(&NSString::from_str(&if total == 0 {
                String::new()
            } else {
                format!("{index} / {total}")
            }));
    }

    /// 站点按钮整组重建（换模式 / 改设置后调用）
    pub fn reload_sites(&self, mtm: MainThreadMarker, sites: &[SiteTarget]) {
        for pill in self.sites.borrow().iter() {
            pill.view().removeFromSuperview();
        }
        let mut built = Vec::with_capacity(sites.len());
        for (index, site) in sites.iter().enumerate() {
            let pill = views::neutral_pill(mtm, &site.name, views::rect(0.0, 0.0, 40.0, CTRL_H));
            pill.button()
                .setToolTip(Some(&NSString::from_str(&site.url)));
            pill.button().setTag(index as isize);
            self.sites_row.addSubview(pill.view());
            built.push(pill);
        }
        *self.sites.borrow_mut() = built;
    }

    pub fn sites(&self) -> Vec<Pill> {
        self.sites.borrow().clone()
    }

    pub fn browser_row_count(&self) -> usize {
        self.browser_doc.subviews().count()
    }

    pub fn set_redact_on(&self, on: bool) {
        self.set_redact.setState(if on {
            NSControlStateValueOn
        } else {
            NSControlStateValueOff
        });
    }

    pub fn set_copy_title(&self, title: &str) {
        self.copy.set_title(title);
    }

    /// 「已复制 ✓」期间把主按钮提亮一档：CSS 那边是 .done{filter:brightness(1.2)}，
    /// 这里只能改填充色 —— alphaValue 在这套层级里不参与合成（实测）。
    pub fn set_copy_done(&self, done: bool) {
        let fill = if done {
            views::rgba(64.0, 150.0, 255.0, 1.0)
        } else {
            views::accent()
        };
        self.copy.set_fill(&fill);
    }

    /// 状态行按 CSS 的 .line-slot 固定占位（16pt），空着也不撤行：
    /// 「组装中 → 完成 → 收回」来回改高度会让整块正文上下跳，比留白更糟
    pub fn set_ctx_status(&self, text: &str, error: bool) {
        views::set_status(&self.ctx_status, text, text, error);
    }

    pub fn set_pack_meta(&self, text: &str, tip: &str) {
        views::set_status(&self.pack_meta, text, tip, false);
    }

    /// 三角跟着开合换字形：CSS 用 rotate(90deg)，NSButton 的标题没有 transform
    pub fn set_settings_open(&self, open: bool) {
        let glyph = if open { "\u{25BE}" } else { "\u{25B8}" };
        views::set_title(&self.settings_toggle, &format!("{glyph}  设置"));
    }

    pub fn site_index_of(&self, sender: &NSButton) -> Option<usize> {
        let tag = sender.tag();
        (tag >= 0).then_some(tag as usize)
    }

    /// 浏览器条目：整行可点，两行文字 ——
    /// 第一行 `✓ agent · #会话id` + 名称（吃掉剩余宽度）+ 相对时间，第二行首条消息摘要；
    /// 选中的那条除了打勾还铺一层侧栏蓝底（CSS: .br-item.sel），不靠颜色单独表意。
    /// 认「同一条会话」要比 (文件, 会话 id)：QoderWork 的 .db 一个文件里装着几十个会话。
    pub fn show_browser(
        &self,
        mtm: MainThreadMarker,
        refs: &[SessionRef],
        selected: Option<&SessionRef>,
    ) {
        for sub in self.browser_doc.subviews().to_vec() {
            sub.removeFromSuperview();
        }
        let width = self.browser_doc.frame().size.width;
        for (index, reference) in refs.iter().enumerate() {
            let line = flipped_view(
                mtm,
                views::rect(0.0, index as f64 * BROWSER_LINE, width, BROWSER_LINE),
            );
            let picked = Some(reference) == selected;
            if picked {
                let tint = views::card(
                    mtm,
                    views::rect(0.0, 1.0, width - 2.0, BROWSER_LINE - 4.0),
                    R_CTRL,
                    &views::accent_tint(),
                    None,
                );
                line.addSubview(&tint);
            }
            let short_id = reference
                .session_id
                .as_deref()
                .map(|id| format!("#{}", id.chars().take(8).collect::<String>()))
                .unwrap_or_default();
            let name = reference.name.clone().unwrap_or_else(|| {
                reference
                    .project_path
                    .as_deref()
                    .map(|p| p.rsplit(['/', '\\']).next().unwrap_or(p).to_string())
                    .unwrap_or_default()
            });
            let mark = if picked { "\u{2713}  " } else { "" };
            let head = views::label(
                mtm,
                &format!("{mark}{} {short_id}", reference.agent),
                T_META,
                &views::dim(),
                views::rect(S2, 3.0, BROWSER_LEFT, LINE_H),
            );
            let title = views::label(
                mtm,
                &name,
                T_BODY,
                &views::ink(),
                views::rect(
                    S2 + BROWSER_LEFT + S1,
                    2.0,
                    width - 2.0 * (S2 + BROWSER_LEFT + S1) - BROWSER_TIME,
                    LINE_H,
                ),
            );
            let time = views::label(
                mtm,
                &short_time(reference.mtime_ms),
                T_META,
                &views::faint(),
                views::rect(width - S2 - BROWSER_TIME, 3.0, BROWSER_TIME, LINE_H),
            );
            time.setAlignment(NSTextAlignment::Right);
            let preview = views::label(
                mtm,
                reference.preview.as_deref().unwrap_or(""),
                T_META,
                &views::dim(),
                views::rect(S2, 20.0, width - 2.0 * S2, LINE_H - 2.0),
            );
            let hit = views::glyph_button(mtm, "", views::rect(0.0, 0.0, width, BROWSER_LINE));
            hit.setToolTip(Some(&NSString::from_str(&row_tip(reference))));
            hit.setTag(index as isize);
            for view in [&head as &NSView, &title, &time, &preview, &hit] {
                line.addSubview(view);
            }
            self.browser_doc.addSubview(&line);
        }
        let content = refs.len() as f64 * BROWSER_LINE;
        self.browser_doc
            .setFrameSize(NSSize::new(width, content.max(BROWSER_LINE)));
        self.size_browser(content.min(BROWSER_MAX));
    }

    /// 盒子高度 = 列表可视高 + 上下各 2pt 呼吸（列表文字不再顶到圆角边）
    fn size_browser(&self, visible: f64) {
        let height = if visible <= 0.0 { 0.0 } else { visible + 4.0 };
        self.browser_box.setFrameSize(NSSize::new(BODY_W, height));
        self.row(Row::Browser).height.set(height);
    }

    /// 「正在发现会话…」/「未发现任何可解析的会话」这类占位行
    pub fn set_browser_note(&self, mtm: MainThreadMarker, message: &str) {
        let note = label(mtm, message, BODY_W - 16.0);
        note.setFrameOrigin(NSPoint::new(8.0, 8.0));
        self.browser_doc.addSubview(&note);
        self.size_browser(BROWSER_LINE);
    }

    /// 浏览器第 index 行的整行热区按钮（tag = index）
    pub fn browser_button(&self, index: usize) -> Option<Retained<NSButton>> {
        let rows = self.browser_doc.subviews();
        if index >= rows.count() {
            return None;
        }
        let buttons = rows.objectAtIndex(index).subviews();
        (0..buttons.count()).find_map(|i| buttons.objectAtIndex(i).downcast::<NSButton>().ok())
    }

    /// 阻塞主线程做重活之前先把它画出来，否则用户看不到「正在填充上下文…」
    pub fn redraw(&self) {
        self.blur.display();
    }

    /// 系统开了「减弱动态效果」就不动，和 CSS 的 prefers-reduced-motion 一样。
    pub fn reduce_motion() -> bool {
        NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion()
    }

    /// 展开时重放入场：淡入 + 从下方 6pt 抬起来，220ms，decelerate。
    /// 原来的 180ms + 4px + 默认 ease-in-out 起步太软（面板是「被叫出来」的，不是飘进来的）。
    pub fn play_enter(&self) {
        let target = self.window.frame();
        if Self::reduce_motion() {
            self.window.setAlphaValue(1.0);
            return;
        }
        // 入场那 6pt 的偏移是动画起点，不是窗口位置：期间必须挡住 persist，
        // 否则 setFrame 触发的 windowDidMove 会把偏掉的坐标写进 settings.json
        self.animating.set(true);
        self.window.setAlphaValue(0.0);
        self.window.setFrame_display(
            views::rect(
                target.origin.x,
                target.origin.y - views::MOTION_RISE,
                target.size.width,
                target.size.height,
            ),
            false,
        );
        let window = self.window.clone();
        let curve = views::ease_out();
        let changes = RcBlock::new(move |ctx: NonNull<NSAnimationContext>| unsafe {
            ctx.as_ref().setDuration(views::MOTION_IN);
            ctx.as_ref().setTimingFunction(Some(&curve));
            let animator: Retained<AnyObject> = msg_send![&window, animator];
            let _: () = msg_send![&animator, setAlphaValue: 1.0];
            let _: () = msg_send![&animator, setFrame: target, display: true];
        });
        let done = self.animating.clone();
        let finished = RcBlock::new(move || done.set(false));
        NSAnimationContext::runAnimationGroup_completionHandler(&changes, Some(&finished));
    }

    // 折叠时不做退场动画：那是「别挡我」的手势，120ms 的淡出只会让它变慢，
    // 而且淡出中途再展开会把面板留在 alpha 0（试过，故砍掉）。

    /// 动画期间（入场 / 长高变矮）不要持久化窗口坐标
    pub fn is_animating(&self) -> bool {
        self.animating.get()
    }

    /// 逐帧高度过渡由 Controller 的计时器驱动，这里只是把「别存坐标」的门开合
    pub fn set_animating(&self, on: bool) {
        self.animating.set(on);
    }

    pub fn content_height(&self) -> f64 {
        let rows: Vec<(Row, f64)> = self
            .rows
            .iter()
            .filter(|r| r.on.get())
            .map(|r| (r.kind, r.height.get()))
            .collect();
        stack_height(&rows)
    }

    /// 头部 + 正文外框占掉的高度
    fn chrome() -> f64 {
        PAD_TOP + HEAD_H + ROW_GAP + PAD_BOTTOM
    }

    /// 当前正文可视高度（动画的起点）
    pub fn body(&self) -> f64 {
        self.scroll.frame().size.height
    }

    /// 内容要多少正文高度（已经夹过 240 下限与工作区）
    pub fn wanted_body(&self, geometry: &Geometry, anchor: (f64, f64)) -> f64 {
        let available = geometry
            .display_near(anchor.0, anchor.1)
            .map_or(f64::MAX, |d| d.work.y + d.work.h - anchor.1 - EDGE);
        body_height(self.content_height(), available - Self::chrome())
    }

    /// 整窗高度 = 头部 + 正文（夹在 [下限, 工作区可用] 之间），然后重排
    pub fn arrange(&self, geometry: &Geometry, anchor: (f64, f64), wanted: f64) -> f64 {
        let chrome = Self::chrome();
        // Electron 的 autoHeight 用的是 getDisplayNearestPoint(窗口左上角)，
        // 取错屏幕会让副屏上的面板按主屏高度夹（副屏更高被截短 / 更矮则整块沉出屏外）
        let available = geometry
            .display_near(anchor.0, anchor.1)
            .map_or(f64::MAX, |d| d.work.y + d.work.h - anchor.1 - EDGE);
        let body = body_height(wanted, available - chrome);
        let height = chrome + body;

        let head_bottom = height - PAD_TOP - HEAD_H;
        let icon_y = head_bottom + (HEAD_H - ICON) / 2.0;
        self.card.setFrame(views::rect(0.0, 0.0, WIDTH, height));
        self.collapse
            .setFrame(views::rect(PAD_X, icon_y, ICON, ICON));
        self.quit
            .setFrame(views::rect(WIDTH - PAD_X - ICON, icon_y, ICON, ICON));
        let capture_w = self.capture.frame().size.width.max(74.0);
        // 未读点固定摆在按钮左侧的槽里，和 chip 同一个位置：
        // 以前它压在按钮右上角，蓝点落在蓝底上等于没点亮（走查 E4）
        let capture_x = WIDTH - PAD_X - ICON - GAP - DOT - GAP - capture_w;
        self.capture
            .set_frame(views::rect(capture_x, head_bottom, capture_w, CTRL_H));
        self.unread.setFrame(views::rect(
            capture_x + capture_w + GAP,
            head_bottom + (CTRL_H - DOT) / 2.0,
            DOT,
            DOT,
        ));
        let status_x = PAD_X + ICON + GAP;
        self.status.setFrame(views::rect(
            status_x,
            head_bottom + (HEAD_H - LINE_H) / 2.0,
            capture_x - GAP - status_x,
            LINE_H,
        ));

        self.scroll
            .setFrame(views::rect(PAD_X, PAD_BOTTOM, BODY_W, body));
        self.layout_rows();
        let origin = geometry.cocoa_origin(anchor.0, anchor.1, height);
        self.window
            .setFrame_display(views::rect(origin.x, origin.y, WIDTH, height), true);
        height
    }

    /// 按内容重算需要多高（Electron 的 autoHeight：内容变了就重新量）
    pub fn arrange_to_content(&self, geometry: &Geometry, anchor: (f64, f64)) -> f64 {
        self.layout_rows();
        let wanted = self.content_height();
        self.arrange(geometry, anchor, wanted)
    }

    /// 动画帧：把正文高度摆到 `body`（不做内容自适应），行列位置照常重排。
    pub fn arrange_body(&self, geometry: &Geometry, anchor: (f64, f64), body: f64) {
        self.layout_rows();
        self.arrange(geometry, anchor, body);
    }

    /// 可见行从上往下排；每行内部先自己排一遍（高度可能因此变化）
    fn layout_rows(&self) {
        let mut y = 0.0;
        let mut above: Option<Row> = None;
        for spec in &self.rows {
            if !spec.on.get() {
                continue;
            }
            match spec.kind {
                Row::Mode => self.layout_mode(),
                Row::CtxActions => self.layout_ctx_actions(),
                Row::Actions => spec.height.set(self.layout_actions()),
                Row::Editor => spec.height.set(self.layout_editor()),
                Row::Browser => {
                    let h = spec.height.get();
                    self.browser_box.setFrame(views::rect(0.0, 0.0, BODY_W, h));
                }
                _ => {}
            }
            if let Some(previous) = above {
                y += row_gap(previous, spec.kind);
            }
            // 连尺寸一起设：只挪 origin 的话，换行撑高的行（动作条 / 设置编辑器）
            // 实际 frame 还是创建时那点高度，点下面半截就穿到 doc 上，命不中控件
            spec.view
                .setFrame(views::rect(0.0, y, BODY_W, spec.height.get()));
            y += spec.height.get();
            above = Some(spec.kind);
        }
        self.doc.setFrameSize(NSSize::new(BODY_W, y));
    }

    /// 选中段填 accent，未选段退回轨道同色（等于看不见，只剩文字）
    /// 轨道内容面的可用矩形：与轨道的可见边框重合（[`views::face`] 补掉 NSBox 缩进）
    fn mode_face(&self) -> NSRect {
        self.mode_track
            .contentView()
            .map_or(views::rect(0.0, 0.0, MODE_W, CTRL_H), |_| {
                views::face(&self.mode_track)
            })
    }

    /// 选中那一段 = 滑块停在那儿。只有模式真的变了才动：启动时 sync_from_state
    /// 也会走到这里，那时看到滑块从左边飞过来就是 bug。
    fn paint_mode(&self, read: bool) {
        let face = self.mode_face();
        let seg_w = (face.size.width - 3.0 * SEG_INSET) / 2.0;
        let target = views::rect(
            if read {
                SEG_INSET
            } else {
                2.0 * SEG_INSET + seg_w
            },
            SEG_INSET,
            seg_w,
            face.size.height - 2.0 * SEG_INSET,
        );
        let changed = self.mode_shown.get() != read;
        self.mode_shown.set(read);
        if !changed || Self::reduce_motion() {
            self.mode_thumb.setFrame(target);
            return;
        }
        let thumb = self.mode_thumb.clone();
        let curve = views::ease_out();
        let changes = RcBlock::new(move |ctx: NonNull<NSAnimationContext>| unsafe {
            ctx.as_ref().setDuration(views::MOTION_SNAP);
            ctx.as_ref().setTimingFunction(Some(&curve));
            let animator: Retained<AnyObject> = msg_send![&thumb, animator];
            let _: () = msg_send![&animator, setFrame: target];
        });
        NSAnimationContext::runAnimationGroup(&changes);
    }

    fn layout_mode(&self) {
        // 段宽按轨道内容面算，标签与滑块共用同一套坐标
        let face = self.mode_face();
        let seg_w = (face.size.width - 3.0 * SEG_INSET) / 2.0;
        let seg_h = face.size.height - 2.0 * SEG_INSET;
        for (index, label) in [&self.mode_read, &self.mode_direct].iter().enumerate() {
            let x = SEG_INSET + index as f64 * (seg_w + SEG_INSET);
            label.setFrame(views::rect(x, SEG_INSET, seg_w, seg_h));
        }
        self.paint_mode(self.mode_shown.get());
        // selects 不能再铺满整行：它是透明的，但照样吃 hitTest，会把左边的模式轨道盖住
        let x = MODE_W + GAP;
        self.selects
            .setFrame(views::rect(x, 0.0, BODY_W - x, CTRL_H));
        self.agent_pick.setFrameOrigin(NSPoint::new(0.0, 0.0));
        self.turns_pick
            .setFrameOrigin(NSPoint::new(AGENT_W + GAP, 0.0));
    }

    fn layout_ctx_actions(&self) {
        let browse_w = self.browse.place(0.0, 0.0, 60.0);
        self.refresh.place(browse_w + S2, 0.0, 60.0);
    }

    fn layout_actions(&self) -> f64 {
        let sites_ref = self.sites.borrow();
        let mut count = 0;
        for site in sites_ref.iter() {
            let width = site.fit_width(40.0);
            site.set_frame(views::rect(0.0, 0.0, width, CTRL_H));
            count += 1;
        }
        let sites: Vec<&NSView> = sites_ref.iter().map(|p| p.view()).collect();
        // #sites 自己是会换行的，站点多起来时不能把它们挤出右边界
        let sites_height = flow(&sites, BODY_W);
        self.sites_row
            .setFrameSize(NSSize::new(BODY_W, sites_height));

        let copy_w = self.copy.fit_width(74.0);
        self.copy.set_frame(views::rect(0.0, 0.0, copy_w, CTRL_H));

        let mut items: Vec<&NSView> = Vec::new();
        if !self.prompt_pick.isHidden() {
            items.push(&self.prompt_pick);
        }
        items.push(self.copy.view());
        if count > 0 {
            items.push(&self.sites_row);
        }
        flow(&items, BODY_W)
    }

    fn layout_editor(&self) -> f64 {
        // 指令条：弹框 + 新建/删除 + 「n/共 m 条」，放不下换行，行高由 flow 决定
        for pill in [&self.pe_new, &self.pe_del] {
            let width = pill.fit_width(40.0);
            pill.set_frame(views::rect(0.0, 0.0, width, CTRL_H));
        }
        self.pe_count.setFrameSize(NSSize::new(64.0, CTRL_H));
        // 开关的实际宽度随系统版本变，标签只能跟着它排，不能写死 44
        let switch_w = self.set_redact.frame().size.width;
        self.redact_label
            .setFrameOrigin(NSPoint::new(switch_w + S2, (CTRL_H - LINE_H) / 2.0));
        let bar = flow(
            &[
                &self.pe_pick as &NSView,
                self.pe_new.view(),
                self.pe_del.view(),
            ],
            BODY_W,
        );
        // #pe-count { margin-left: auto }：计数贴右
        self.pe_count
            .setFrameOrigin(NSPoint::new(BODY_W - 64.0, (CTRL_H - LINE_H) / 2.0));
        self.pe_bar.setFrameSize(NSSize::new(BODY_W, bar));

        let mut y = 0.0;
        if !self.editor_ctx.isHidden() {
            for child in self.editor_ctx.subviews().to_vec() {
                child.setFrameOrigin(NSPoint::new(0.0, y));
                if let Some(card) = child.downcast_ref::<NSBox>() {
                    refit_box(card);
                }
                y += child.frame().size.height + ROW_GAP;
            }
            self.editor_ctx
                .setFrameSize(NSSize::new(BODY_W, (y - ROW_GAP).max(0.0)));
        }
        for view in [&self.sites_label as &NSView, &self.sites_box] {
            view.setFrameOrigin(NSPoint::new(0.0, y));
            y += view.frame().size.height + ROW_GAP;
        }
        refit_box(&self.sites_box);
        // #set-save { margin-left: auto }：贴右，读起来是一个动作，不是又一块草稿框
        let save_w = self.set_save.fit_width(72.0);
        self.set_save
            .set_frame(views::rect(BODY_W - save_w, y, save_w, CTRL_H));
        y + CTRL_H
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visible_rows_stack_with_gaps() {
        assert_eq!(stack_height(&[]), 0.0);
        assert_eq!(stack_height(&[(Row::Mode, CTRL_H)]), CTRL_H);
        assert_eq!(
            stack_height(&[(Row::Mode, CTRL_H), (Row::CtxStatus, LINE_H)]),
            CTRL_H + LINE_H + GROUP_GAP,
            "同组两行之间是组内距"
        );
        assert_eq!(
            stack_height(&[
                (Row::CtxStatus, LINE_H),
                (Row::DivAct, views::SECTION_LINE),
                (Row::Actions, CTRL_H)
            ]),
            LINE_H + views::SECTION_LINE + CTRL_H + ROW_GAP * 2.0,
            "发丝行两边都按组间距算：它标的就是分组边界"
        );
    }

    #[test]
    fn body_follows_content_and_respects_the_work_area() {
        assert_eq!(
            body_height(100.0, 1000.0),
            100.0,
            "内容多高就多高：直通模式没有地板把它撑出一条底部空白"
        );
        assert_eq!(body_height(500.0, 1000.0), 500.0);
        assert_eq!(
            body_height(1000.0, 300.0),
            300.0,
            "超出工作区就夹住，正文自己滚"
        );
        assert_eq!(
            body_height(1000.0, 20.0),
            MIN_ROOM,
            "屏幕只剩一条缝时保住应急下限，正文别整个消失"
        );
    }
}
