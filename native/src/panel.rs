//! panel 窗口：展开态。头部固定，正文是一叠「行」—— 模式与折叠状态决定哪些行在场，
//! 面板高度由可见行加起来（超过工作区就滚动），所以从不写死。
//! 对照 static/index.html 的 #panel 结构与 renderer.js 的 applyMode()。

use std::cell::{Cell, RefCell};

use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSBorderType, NSBox, NSButton, NSColor, NSControlStateValueOff, NSControlStateValueOn, NSFont,
    NSPanel, NSPopUpButton, NSScrollView, NSSegmentStyle, NSSegmentedControl, NSSwitch,
    NSTextField, NSTextView, NSView, NSVisualEffectView,
};
use objc2_foundation::{NSArray, NSPoint, NSSize, NSString};

use crate::ctxpack::adapters::SessionRef;
use crate::flipped::flipped_view;
use crate::geo::Geometry;
use crate::settings::{PromptTemplate, SessionPath, SiteTarget};
use crate::views::{
    self, BADGE, BUTTON_W, GAP, HEAD_H, ICON, LINE_H, PAD_BOTTOM, PAD_TOP, PAD_X, RADIUS, ROW_GAP,
};

pub const WIDTH: f64 = 400.0;
/// Electron 的 autoHeight 下限
const MIN_BODY: f64 = 240.0;
/// 夹到工作区时留的边距，与 geo::EDGE 同值
const EDGE: f64 = 8.0;
const ROW: f64 = 24.0;
/// 多行编辑框（对应 textarea rows=3~5）
const FIELD: f64 = 66.0;
/// #actions / #sites 的 flex gap
const INNER: f64 = 6.0;
const BROWSER_LINE: f64 = 34.0;
const BROWSER_MAX: f64 = 168.0;
const BODY_W: f64 = WIDTH - 2.0 * PAD_X;

/// 正文里的行。`on` 是 CSS 的 display:none（不占位）；模式内的控件用 setHidden（占位不画），
/// 两种语义分开，切换时才不会跳版。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Row {
    Mode,
    CtxActions,
    Session,
    CtxStatus,
    Browser,
    Actions,
    PackMeta,
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
    pub capture: Retained<NSButton>,
    pub quit: Retained<NSButton>,
    pub unread: Retained<NSBox>,
    // ---- 正文 ----
    scroll: Retained<NSScrollView>,
    doc: Retained<NSView>,
    rows: Vec<RowSpec>,
    pub mode: Retained<NSSegmentedControl>,
    pub selects: Retained<NSView>,
    pub agent_pick: Retained<NSPopUpButton>,
    pub turns_pick: Retained<NSPopUpButton>,
    pub browse: Retained<NSButton>,
    pub refresh: Retained<NSButton>,
    pub session_line: Retained<NSTextField>,
    pub ctx_status: Retained<NSTextField>,
    browser_scroll: Retained<NSScrollView>,
    pub browser_doc: Retained<NSView>,
    pub prompt_pick: Retained<NSPopUpButton>,
    pub copy: Retained<NSButton>,
    pub sites_row: Retained<NSView>,
    pub pack_meta: Retained<NSTextField>,
    pub settings_toggle: Retained<NSButton>,
    pub editor_ctx: Retained<NSView>,
    pe_bar: Retained<NSView>,
    pub pe_pick: Retained<NSPopUpButton>,
    pub pe_new: Retained<NSButton>,
    pub pe_del: Retained<NSButton>,
    pub pe_count: Retained<NSTextField>,
    pub pe_tpl: Retained<NSTextView>,
    pub set_sessions: Retained<NSTextView>,
    pub set_redact: Retained<NSSwitch>,
    redact_label: Retained<NSTextField>,
    pub sites_label: Retained<NSTextField>,
    pub set_sites: Retained<NSTextView>,
    pub set_save: Retained<NSButton>,
    sites_scroll: Retained<NSScrollView>,
    sites: RefCell<Vec<Retained<NSButton>>>,
}

/// 正文高度 = 可见行高之和 + 行间距（Electron 那边量 body.scrollHeight 的等价物）
fn stack_height(heights: &[f64]) -> f64 {
    heights.iter().sum::<f64>() + ROW_GAP * heights.len().saturating_sub(1) as f64
}

/// `room` 是工作区剩下的空间：内容再少也不低于 240 下限，再高也要夹住让正文自己滚
fn body_height(wanted: f64, room: f64) -> f64 {
    wanted.max(MIN_BODY).min(room.max(MIN_BODY))
}

/// 行容器一律 flipped：行内的 y 表示「离行顶多远」，换行时第二行才不会被画到第一行上面
fn container(mtm: MainThreadMarker, height: f64) -> Retained<NSView> {
    flipped_view(mtm, views::rect(0.0, 0.0, BODY_W, height)).into_super()
}

fn label(mtm: MainThreadMarker, title: &str, width: f64) -> Retained<NSTextField> {
    views::label(
        mtm,
        title,
        12.0,
        &NSColor::secondaryLabelColor(),
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
        views::rect(0.0, 0.0, width, ROW),
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

/// 多行编辑框：暗底 + 细边，等价 CSS 的 textarea
fn text_area(
    mtm: MainThreadMarker,
    tip: Option<&str>,
) -> (Retained<NSScrollView>, Retained<NSTextView>) {
    let scroll = NSScrollView::new(mtm);
    scroll.setFrame(views::rect(0.0, 0.0, BODY_W, FIELD));
    scroll.setBorderType(NSBorderType::LineBorder);
    scroll.setHasVerticalScroller(true);
    scroll.setBackgroundColor(&views::rgba(0.0, 0.0, 0.0, 0.28));
    views::set_tip(&scroll, tip);
    let text = NSTextView::new(mtm);
    text.setEditable(true);
    text.setRichText(false);
    text.setBackgroundColor(&views::rgba(0.0, 0.0, 0.0, 0.28));
    text.setFont(Some(&NSFont::systemFontOfSize(12.0)));
    text.setTextColor(Some(&NSColor::labelColor()));
    text.setMinSize(NSSize::new(0.0, 0.0));
    text.setMaxSize(NSSize::new(f64::MAX, f64::MAX));
    scroll.setDocumentView(Some(&text));
    (scroll, text)
}

/// 从左到右摆一排，放不下就换行；返回用掉的总高度（行高取该行最高的那个，站点条会比一行高）
fn flow(views: &[&NSView], avail: f64) -> f64 {
    let mut x = 0.0;
    let mut y = 0.0;
    let mut line = ROW;
    for view in views {
        let width = view.frame().size.width;
        if x > 0.0 && x + width > avail {
            x = 0.0;
            y += line + INNER;
            line = ROW;
        }
        view.setFrameOrigin(NSPoint::new(x, y));
        line = line.max(view.frame().size.height);
        x += width + INNER;
    }
    y + line
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
        let height = MIN_BODY + PAD_TOP + HEAD_H + ROW_GAP + PAD_BOTTOM;
        let window = views::keyable_panel(
            mtm,
            WIDTH,
            height,
            geometry.cocoa_origin(anchor.0, anchor.1, height),
        );

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

        // ---------- 头部行 ----------
        let collapse = views::glyph_button(mtm, "\u{25BE}", views::rect(0.0, 0.0, ICON, ICON));
        let status = label(mtm, crate::capture::NO_SELECTION, 1.0);
        let capture = views::push_button(mtm, "取入选区", views::rect(0.0, 0.0, BUTTON_W, ROW));
        views::set_tip(&capture, Some("把刚才复制的内容取进来"));
        let quit = views::glyph_button(mtm, "\u{2715}", views::rect(0.0, 0.0, ICON, ICON));
        views::set_tip(&quit, Some("退出常驻"));
        let unread = views::card(
            mtm,
            views::rect(0.0, 0.0, BADGE, BADGE),
            BADGE / 2.0,
            &views::rgba(10.0, 132.0, 255.0, 1.0),
            None,
        );
        views::set_dot(&unread, false);
        for view in [&collapse as &NSView, &status, &capture, &quit, &unread] {
            blur.addSubview(view);
        }

        // ---------- 正文滚动区 ----------
        let doc = flipped_view(mtm, views::rect(0.0, 0.0, BODY_W, MIN_BODY));
        let scroll = NSScrollView::new(mtm);
        scroll.setBorderType(NSBorderType::NoBorder);
        scroll.setHasVerticalScroller(true);
        scroll.setAutohidesScrollers(true);
        scroll.setDrawsBackground(false);
        scroll.setDocumentView(Some(&doc));
        blur.addSubview(&scroll);

        // 1 模式行
        let mode = NSSegmentedControl::new(mtm);
        mode.setSegmentCount(2);
        mode.setLabel_forSegment(&NSString::from_str("会话解读"), 0);
        mode.setLabel_forSegment(&NSString::from_str("选区直通"), 1);
        mode.setSegmentStyle(NSSegmentStyle::TexturedRounded);
        mode.setSelectedSegment(if read_mode { 0 } else { 1 });
        let selects = container(mtm, ROW);
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
        let mode_row = container(mtm, ROW);
        mode_row.addSubview(&mode);
        mode_row.addSubview(&selects);

        // 2 上下文动作行
        let browse = views::text_button(mtm, "浏览会话", views::rect(0.0, 0.0, 60.0, ROW));
        views::set_tip(&browse, Some("点一条即用它填充"));
        let refresh = views::text_button(mtm, "刷新上下文", views::rect(0.0, 0.0, 60.0, ROW));
        let ctx_actions = container(mtm, ROW);
        ctx_actions.addSubview(&browse);
        ctx_actions.addSubview(&refresh);

        // 3/4 判定会话 + 状态
        let session_line = label(mtm, "上下文未填充", BODY_W);
        let ctx_status = label(mtm, "", BODY_W);
        let session_row = container(mtm, LINE_H);
        session_row.addSubview(&session_line);
        let status_row = container(mtm, LINE_H);
        status_row.addSubview(&ctx_status);

        // 5 会话浏览器
        let browser_doc = flipped_view(mtm, views::rect(0.0, 0.0, BODY_W - 16.0, BROWSER_LINE));
        let browser_scroll = NSScrollView::new(mtm);
        browser_scroll.setBorderType(NSBorderType::LineBorder);
        browser_scroll.setHasVerticalScroller(true);
        browser_scroll.setBackgroundColor(&views::rgba(0.0, 0.0, 0.0, 0.28));
        browser_scroll.setDocumentView(Some(&browser_doc));
        let browser_row = container(mtm, BROWSER_MAX);
        browser_row.addSubview(&browser_scroll);

        // 6 动作行
        let prompt_pick = popup(mtm, 96.0, &[], Some("增删改在设置里"));
        let copy = views::push_button(mtm, "复制 Prompt", views::rect(0.0, 0.0, BUTTON_W, ROW));
        views::set_tip(&copy, Some("指令 + 选区 + 历史，一次复制"));
        let sites_row = container(mtm, ROW);
        let actions = container(mtm, ROW);
        actions.addSubview(&prompt_pick);
        actions.addSubview(&copy);
        actions.addSubview(&sites_row);

        // 7 字数行
        let pack_meta = label(mtm, "", BODY_W);
        let meta_row = container(mtm, LINE_H);
        meta_row.addSubview(&pack_meta);

        // 8 设置折叠
        let settings_toggle = views::text_button(mtm, "设置", views::rect(0.0, 0.0, 48.0, ROW));
        let settings_row = container(mtm, ROW);
        settings_row.addSubview(&settings_toggle);

        // 9 设置编辑器
        let editor = flipped_view(mtm, views::rect(0.0, 0.0, BODY_W, FIELD));
        let editor_ctx = container(mtm, 0.0);
        let pe_pick = popup(mtm, 120.0, &[], None);
        let pe_new = views::text_button(mtm, "+ 新建", views::rect(0.0, 0.0, 48.0, ROW));
        let pe_del = views::text_button(mtm, "删除", views::rect(0.0, 0.0, 40.0, ROW));
        let pe_count = label(mtm, "", 48.0);
        let pe_bar = container(mtm, ROW);
        pe_bar.addSubview(&pe_pick);
        pe_bar.addSubview(&pe_new);
        pe_bar.addSubview(&pe_del);
        pe_bar.addSubview(&pe_count);
        let (pe_tpl_scroll, pe_tpl) = text_area(mtm, None);
        let sessions_label = label(mtm, "会话路径", BODY_W);
        let (sessions_scroll, set_sessions) = text_area(
            mtm,
            Some("每行：agent|路径；发现只读这里，删一行即停扫该源"),
        );
        let redact_row = container(mtm, ROW);
        let set_redact = NSSwitch::new(mtm);
        set_redact.sizeToFit();
        set_redact.setFrameOrigin(NSPoint::new(
            0.0,
            (ROW - set_redact.frame().size.height) / 2.0,
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
            &pe_tpl_scroll,
            &sessions_label,
            &sessions_scroll,
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
        let (sites_scroll, set_sites) = text_area(mtm, Some("每行：名称|URL"));
        let set_save = views::push_button(mtm, "保存设置", views::rect(0.0, 0.0, 72.0, ROW));
        for view in [
            &editor_ctx as &NSView,
            &sites_label,
            &sites_scroll,
            &set_save,
        ] {
            editor.addSubview(view);
        }

        let rows = vec![
            RowSpec {
                kind: Row::Mode,
                view: mode_row,
                height: Cell::new(ROW),
                on: Cell::new(true),
            },
            RowSpec {
                kind: Row::CtxActions,
                view: ctx_actions,
                height: Cell::new(ROW),
                on: Cell::new(read_mode),
            },
            RowSpec {
                kind: Row::Session,
                view: session_row,
                height: Cell::new(LINE_H),
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
                kind: Row::Actions,
                view: actions,
                height: Cell::new(ROW),
                on: Cell::new(true),
            },
            RowSpec {
                kind: Row::PackMeta,
                view: meta_row,
                height: Cell::new(LINE_H),
                on: Cell::new(true),
            },
            RowSpec {
                kind: Row::Settings,
                view: settings_row,
                height: Cell::new(ROW),
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
            mode,
            selects,
            agent_pick,
            turns_pick,
            browse,
            refresh,
            session_line,
            ctx_status,
            browser_scroll,
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
            set_sites,
            set_save,
            sites_scroll,
            sites: RefCell::new(Vec::new()),
        };
        panel.reload_sites(mtm, sites);
        panel.reload_prompts(prompts, active_prompt);
        panel.set_turns(turns);
        panel.reload_session_paths(session_paths);
        panel.arrange(geometry, anchor, MIN_BODY);
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
        self.mode.setSelectedSegment(if read { 0 } else { 1 });
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
        views::set_tip(&self.copy, read.then_some("指令 + 选区 + 历史，一次复制"));
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
            .get(self.agent_turns_index())
            .copied()
            .unwrap_or(8)
    }

    fn agent_turns_index(&self) -> usize {
        self.turns_pick.indexOfSelectedItem().max(0) as usize
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

    pub fn reload_prompts(&self, prompts: &[PromptTemplate], active: usize) {
        let owned = prompts
            .iter()
            .map(|p| NSString::from_str(&p.name))
            .collect::<Vec<_>>();
        let refs: Vec<&NSString> = owned.iter().map(|s| &**s).collect();
        for pick in [&self.prompt_pick, &self.pe_pick] {
            pick.removeAllItems();
            pick.addItemsWithTitles(&NSArray::from_slice(&refs));
        }
        let index = active.min(prompts.len().saturating_sub(1)) as isize;
        self.prompt_pick.selectItemAtIndex(index);
        self.pe_pick.selectItemAtIndex(index);
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
        for button in self.sites.borrow().iter() {
            button.removeFromSuperview();
        }
        let mut built = Vec::with_capacity(sites.len());
        for (index, site) in sites.iter().enumerate() {
            let button = views::text_button(mtm, &site.name, views::rect(0.0, 0.0, 40.0, ROW));
            button.setToolTip(Some(&NSString::from_str(&site.url)));
            button.setTag(index as isize);
            self.sites_row.addSubview(&button);
            built.push(button);
        }
        *self.sites.borrow_mut() = built;
    }

    pub fn sites(&self) -> Vec<Retained<NSButton>> {
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
        views::set_title(&self.copy, title);
    }

    pub fn set_ctx_status(&self, text: &str, error: bool) {
        views::set_status(&self.ctx_status, text, text, error);
    }

    pub fn site_index_of(&self, sender: &NSButton) -> Option<usize> {
        let tag = sender.tag();
        (tag >= 0).then_some(tag as usize)
    }

    /// 浏览器条目：一行两栏（agent#id · 名称 / 预览），整行可点；选中的那条打勾
    pub fn show_browser(&self, mtm: MainThreadMarker, refs: &[SessionRef], selected: Option<&str>) {
        for sub in self.browser_doc.subviews().to_vec() {
            sub.removeFromSuperview();
        }
        let width = self.browser_doc.frame().size.width;
        for (index, reference) in refs.iter().enumerate() {
            let line = flipped_view(
                mtm,
                views::rect(0.0, index as f64 * BROWSER_LINE, width, BROWSER_LINE),
            );
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
            let mark = if selected == Some(reference.file_path.as_str()) {
                "✓ "
            } else {
                ""
            };
            let head = format!("{mark}{}{} · {}", reference.agent, short_id, name);
            let title = label(mtm, &head, width - 16.0);
            title.setFrameOrigin(NSPoint::new(8.0, 4.0));
            title.setTextColor(Some(&NSColor::labelColor()));
            let preview = label(
                mtm,
                reference.preview.as_deref().unwrap_or(""),
                width - 16.0,
            );
            preview.setFrameOrigin(NSPoint::new(8.0, 20.0));
            let hit = views::glyph_button(mtm, "", views::rect(0.0, 0.0, width, BROWSER_LINE));
            hit.setToolTip(Some(&NSString::from_str(&reference.file_path)));
            hit.setTag(index as isize);
            line.addSubview(&title);
            line.addSubview(&preview);
            line.addSubview(&hit);
            self.browser_doc.addSubview(&line);
        }
        let content = refs.len() as f64 * BROWSER_LINE;
        self.browser_doc
            .setFrameSize(NSSize::new(width, content.max(BROWSER_LINE)));
        let visible = content.min(BROWSER_MAX);
        self.row(Row::Browser)
            .height
            .set(if refs.is_empty() { 0.0 } else { visible + 8.0 });
    }

    /// 浏览器第 index 行的整行热区按钮（tag = index）
    /// 「正在发现会话…」/「未发现任何可解析的会话」这类占位行
    pub fn set_browser_note(&self, mtm: MainThreadMarker, message: &str) {
        let note = label(mtm, message, BODY_W - 16.0);
        note.setFrameOrigin(NSPoint::new(8.0, 6.0));
        self.browser_doc.addSubview(&note);
        self.row(Row::Browser).height.set(BROWSER_LINE + 8.0);
    }

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

    pub fn content_height(&self) -> f64 {
        let on = self.rows.iter().filter(|r| r.on.get()).collect::<Vec<_>>();
        let heights: Vec<f64> = on.iter().map(|r| r.height.get()).collect();
        stack_height(&heights)
    }

    /// 整窗高度 = 头部 + 正文（夹在 [下限, 工作区可用] 之间），然后重排
    pub fn arrange(&self, geometry: &Geometry, anchor: (f64, f64), wanted: f64) -> f64 {
        let chrome = PAD_TOP + HEAD_H + ROW_GAP + PAD_BOTTOM;
        let available = geometry
            .displays
            .first()
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
        let capture_x = WIDTH - PAD_X - ICON - GAP - BUTTON_W;
        self.capture
            .setFrame(views::rect(capture_x, head_bottom, BUTTON_W, ROW));
        self.unread.setFrame(views::rect(
            capture_x + BUTTON_W - BADGE + 3.0,
            head_bottom + ROW - BADGE + 3.0,
            BADGE,
            BADGE,
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
        views::set_mask(&self.blur, WIDTH, height);
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

    /// 可见行从上往下排；每行内部先自己排一遍（高度可能因此变化）
    fn layout_rows(&self) {
        let mut y = 0.0;
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
                    self.browser_scroll
                        .setFrame(views::rect(0.0, 0.0, BODY_W, h));
                }
                _ => {}
            }
            // 连尺寸一起设：只挪 origin 的话，换行撑高的行（动作条 / 设置编辑器）
            // 实际 frame 还是创建时那点高度，点下面半截就穿到 doc 上，命不中控件
            spec.view
                .setFrame(views::rect(0.0, y, BODY_W, spec.height.get()));
            y += spec.height.get() + ROW_GAP;
        }
        self.doc
            .setFrameSize(NSSize::new(BODY_W, (y - ROW_GAP).max(0.0)));
    }

    fn layout_mode(&self) {
        self.mode.setFrame(views::rect(0.0, 0.0, 148.0, ROW));
        // selects 不能再铺满整行：它是透明的，但照样吃 hitTest，会把左边的分段控件盖住
        let x = 148.0 + GAP;
        self.selects.setFrame(views::rect(x, 0.0, BODY_W - x, ROW));
        self.agent_pick.setFrameOrigin(NSPoint::new(0.0, 0.0));
        self.turns_pick
            .setFrameOrigin(NSPoint::new(112.0 + GAP, 0.0));
    }

    fn layout_ctx_actions(&self) {
        self.browse.sizeToFit();
        let browse_w = self.browse.frame().size.width.max(60.0);
        self.browse.setFrame(views::rect(0.0, 0.0, browse_w, ROW));
        self.refresh.sizeToFit();
        self.refresh.setFrame(views::rect(
            browse_w + INNER,
            0.0,
            self.refresh.frame().size.width.max(60.0),
            ROW,
        ));
    }

    fn layout_actions(&self) -> f64 {
        let mut buttons = Vec::new();
        for button in self.sites.borrow().iter() {
            button.sizeToFit();
            let width = button.frame().size.width.max(40.0);
            button.setFrameSize(NSSize::new(width, ROW));
            buttons.push(button.clone());
        }
        let sites: Vec<&NSView> = buttons.iter().map(|b| b as &NSView).collect();
        // #sites 自己是会换行的，站点多起来时不能把它们挤出右边界
        let sites_height = flow(&sites, BODY_W);
        self.sites_row
            .setFrameSize(NSSize::new(BODY_W, sites_height));

        self.copy.sizeToFit();
        let copy_w = self.copy.frame().size.width.max(BUTTON_W);
        self.copy.setFrameSize(NSSize::new(copy_w, ROW));

        let mut items: Vec<&NSView> = Vec::new();
        if !self.prompt_pick.isHidden() {
            items.push(&self.prompt_pick);
        }
        items.push(&self.copy);
        if !buttons.is_empty() {
            items.push(&self.sites_row);
        }
        flow(&items, BODY_W)
    }

    fn layout_editor(&self) -> f64 {
        // 指令条：弹框 + 新建/删除 + 「n/共 m 条」，放不下换行，行高由 flow 决定
        for button in [&self.pe_new, &self.pe_del] {
            button.sizeToFit();
            let width = button.frame().size.width.max(40.0);
            button.setFrameSize(NSSize::new(width, ROW));
        }
        self.pe_count.setFrameSize(NSSize::new(64.0, ROW));
        // 开关的实际宽度随系统版本变，标签只能跟着它排，不能写死 44
        let switch_w = self.set_redact.frame().size.width;
        self.redact_label
            .setFrameOrigin(NSPoint::new(switch_w + INNER, (ROW - LINE_H) / 2.0));
        let bar = flow(
            &[&self.pe_pick as &NSView, &self.pe_new, &self.pe_del],
            BODY_W,
        );
        // #pe-count { margin-left: auto }：计数贴右
        self.pe_count
            .setFrameOrigin(NSPoint::new(BODY_W - 64.0, (ROW - LINE_H) / 2.0));
        self.pe_bar.setFrameSize(NSSize::new(BODY_W, bar));

        let mut y = 0.0;
        if !self.editor_ctx.isHidden() {
            for child in self.editor_ctx.subviews().to_vec() {
                child.setFrameOrigin(NSPoint::new(0.0, y));
                y += child.frame().size.height + ROW_GAP;
            }
            self.editor_ctx
                .setFrameSize(NSSize::new(BODY_W, (y - ROW_GAP).max(0.0)));
        }
        for view in [
            &self.sites_label as &NSView,
            &self.sites_scroll,
            &self.set_save,
        ] {
            view.setFrameOrigin(NSPoint::new(0.0, y));
            y += view.frame().size.height + ROW_GAP;
        }
        (y - ROW_GAP).max(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visible_rows_stack_with_gaps() {
        assert_eq!(stack_height(&[]), 0.0);
        assert_eq!(stack_height(&[ROW]), ROW);
        assert_eq!(
            stack_height(&[ROW, LINE_H, ROW]),
            ROW * 2.0 + LINE_H + ROW_GAP * 2.0
        );
    }

    #[test]
    fn body_keeps_the_floor_and_respects_the_work_area() {
        assert_eq!(
            body_height(100.0, 1000.0),
            MIN_BODY,
            "内容再少也有 240 的下限，切模式不跳版"
        );
        assert_eq!(body_height(500.0, 1000.0), 500.0);
        assert_eq!(
            body_height(1000.0, 300.0),
            300.0,
            "超出工作区就夹住，正文自己滚"
        );
        assert_eq!(
            body_height(1000.0, 100.0),
            MIN_BODY,
            "夹到底都比下限窄时保下限"
        );
    }
}
