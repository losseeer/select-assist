//! 双窗调度 + 直通闭环 + 会话解读。
//! 对照 main/index.ts（expand/collapse/轮询/站点）与 static/renderer.js（模式切换、上下文挂载、
//! 会话浏览器、设置编辑器）。界面一律从模型重算，控件只负责显示。

use std::cell::RefCell;
use std::ptr::NonNull;
use std::time::Instant;

use block2::RcBlock;
use objc2::rc::{Retained, Weak};
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{define_class, msg_send, sel, DeclaredClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSApplication, NSButton, NSWindow, NSWindowDelegate};
use objc2_foundation::{NSNotification, NSObject, NSObjectProtocol, NSTimer};

use crate::capture::{self, ClipNote};
use crate::chip::{self, Chip};
use crate::context::{self, Pack, Payload};
use crate::ctxpack::adapters::SessionRef;
use crate::geo::{Geometry, Rect};
use crate::panel::{Panel, Row};
use crate::pasteboard;
use crate::settings::{AppSettings, PromptTemplate, SessionPath, Settings, SiteTarget};
use crate::views;

/// Electron 的轮询周期
const POLL_SECONDS: f64 = 0.8;
const ERROR_HOLD: f64 = 3.0;
/// 成功提示的停留时间，对齐 renderer.js flashStatus 的 4s
const FLASH_HOLD: f64 = 4.0;
const COPIED_HOLD: f64 = 1.2;
const COPY_READ: &str = "复制 Prompt";
const COPY_DIRECT: &str = "复制选区原文";
const COPIED: &str = "已复制 ✓";
/// 会话路径行的合法 agent token（与 renderer.js 的 AGENT_TOKENS 一致）
const AGENT_TOKENS: [&str; 6] = [
    "auto",
    "claude-code",
    "codex",
    "workbuddy",
    "qoder",
    "project",
];

#[derive(Clone, Copy)]
enum After {
    ClearFlash,
    ResetCopyLabel,
}

/// 高度过渡的节拍与总时长：60Hz × ~167ms，落在 motion-plan 的
/// 「面板级状态变化 200ms 上下」这一档
const ANIM_TICK: f64 = 1.0 / 60.0;
const ANIM_SECONDS: f64 = 10.0 * ANIM_TICK;

/// 一次高度过渡的两端（正文高度，单位 pt）与起点时刻。
/// 位置按「已经走了多久」算，不按第几帧算：计时器一定不准，
/// 按帧号走会让动画被拉慢或抽稀，按时间走则只是采样点变了、曲线不变。
#[derive(Clone, Copy)]
struct Anim {
    from: f64,
    to: f64,
    started: Instant,
}

struct State {
    app: AppSettings,
    pack: Pack,
    payload: Option<Payload>,
    unread: Option<ClipNote>,
    last_clip: String,
    last_change: isize,
    error: Option<String>,
    /// 手动挑的会话；None = 按 agent/轮数自动判定
    browsing: Option<SessionRef>,
    browser_refs: Vec<SessionRef>,
    settings_open: bool,
    /// 设置编辑器里的草稿
    prompts: Vec<PromptTemplate>,
    edit_index: usize,
    /// 两个模式各自记住没保存的站点文本
    sites_draft: [String; 2],
}

pub struct Ivars {
    mtm: MainThreadMarker,
    geometry: Geometry,
    file: Settings,
    chip: Chip,
    panel: Panel,
    state: RefCell<State>,
    /// 正在进行的高度过渡（None = 没在动）
    anim: RefCell<Option<Anim>>,
    /// 驱动它的那一条 repeating 计时器（整段动画只挂一次）
    anim_timer: RefCell<Option<Retained<NSTimer>>>,
    weak: RefCell<Option<Weak<Controller>>>,
}

define_class! {
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = Ivars]
    pub struct Controller;

    impl Controller {
        #[unsafe(method(expand:))]
        fn on_expand(&self, _sender: Option<&AnyObject>) {
            self.show(true);
        }

        #[unsafe(method(collapse:))]
        fn on_collapse(&self, _sender: Option<&AnyObject>) {
            self.show(false);
        }

        #[unsafe(method(capture:))]
        fn on_capture(&self, _sender: Option<&AnyObject>) {
            self.capture_selection();
        }

        #[unsafe(method(copy:))]
        fn on_copy(&self, _sender: Option<&AnyObject>) {
            self.copy_payload();
        }

        #[unsafe(method(quit:))]
        fn on_quit(&self, _sender: Option<&AnyObject>) {
            NSApplication::sharedApplication(self.ivars().mtm).terminate(None);
        }

        #[unsafe(method(openSite:))]
        fn on_open_site(&self, sender: Option<&AnyObject>) {
            let Some(button) = sender.and_then(|s| s.downcast_ref::<NSButton>()) else { return };
            let Some(site) = self.ivars().panel.site_index_of(button).and_then(|index| self.current_sites().get(index).cloned()) else {
                return;
            };
            if !pasteboard::open_site(&site.url) {
                eprintln!("select-assist-native: 打不开站点「{}」", site.name);
            }
        }

        /// 两段各自带 tag（0=会话解读 / 1=选区直通），点哪段就是哪个模式
        #[unsafe(method(modeChanged:))]
        fn on_mode(&self, sender: Option<&AnyObject>) {
            let read = sender
                .and_then(|s| s.downcast_ref::<NSButton>())
                .is_none_or(|b| b.tag() == 0);
            self.set_mode(read);
        }

        #[unsafe(method(promptChanged:))]
        fn on_prompt(&self, _sender: Option<&AnyObject>) {
            let active = self.ivars().panel.active_prompt();
            self.ivars().state.borrow_mut().app.active_prompt = active;
            self.persist_settings();
            // 换指令必须重算 payload：Electron 那侧 pack:current / pack:copy 每次都带
            // settings.get() 现算，native 的 payload 是缓存的，不重算就会复制上一条指令的成品
            self.recompute_payload();
            self.relayout();
        }

        #[unsafe(method(peChanged:))]
        fn on_pe_pick(&self, _sender: Option<&AnyObject>) {
            self.commit_template();
            let index = self.ivars().panel.pe_pick.indexOfSelectedItem().max(0) as usize;
            self.ivars().state.borrow_mut().edit_index = index;
            self.reload_prompt_editor();
        }

        #[unsafe(method(browse:))]
        fn on_browse(&self, _sender: Option<&AnyObject>) {
            self.toggle_browser();
        }

        #[unsafe(method(refreshContext:))]
        fn on_refresh(&self, _sender: Option<&AnyObject>) {
            self.attach(None);
        }

        #[unsafe(method(pickSession:))]
        fn on_pick_session(&self, sender: Option<&AnyObject>) {
            let Some(button) = sender.and_then(|s| s.downcast_ref::<NSButton>()) else { return };
            let index = button.tag().max(0) as usize;
            let Some(reference) = self.ivars().state.borrow().browser_refs.get(index).cloned() else {
                return;
            };
            self.ivars().state.borrow_mut().browsing = Some(reference.clone());
            self.ivars().panel.set_agent_token(&reference.agent);
            self.attach(Some(&reference));
            self.ivars().panel.set_row(Row::Browser, false);
            self.relayout();
        }

        #[unsafe(method(toggleSettings:))]
        fn on_toggle_settings(&self, _sender: Option<&AnyObject>) {
            let open = {
                let mut state = self.ivars().state.borrow_mut();
                state.settings_open = !state.settings_open;
                state.settings_open
            };
            self.ivars().panel.set_row(Row::Editor, open);
            self.ivars().panel.set_settings_open(open);
            self.relayout();
        }

        #[unsafe(method(newPrompt:))]
        fn on_new_prompt(&self, _sender: Option<&AnyObject>) {
            self.commit_template();
            {
                let mut state = self.ivars().state.borrow_mut();
                let mut n = state.prompts.len() + 1;
                while state.prompts.iter().any(|p| p.name == format!("指令 {n}")) {
                    n += 1;
                }
                state.prompts.push(PromptTemplate { name: format!("指令 {n}"), template: "{selection}".into() });
                state.edit_index = state.prompts.len() - 1;
            }
            self.reload_prompt_editor();
        }

        #[unsafe(method(delPrompt:))]
        fn on_del_prompt(&self, _sender: Option<&AnyObject>) {
            self.commit_template();
            {
                let mut state = self.ivars().state.borrow_mut();
                if state.prompts.len() <= 1 {
                    drop(state);
                    self.flash("至少保留一条指令");
                    return;
                }
                let index = state.edit_index;
                state.prompts.remove(index);
                state.edit_index = state.edit_index.saturating_sub(1);
            }
            self.reload_prompt_editor();
        }

        #[unsafe(method(saveSettings:))]
        fn on_save_settings(&self, _sender: Option<&AnyObject>) {
            self.save_settings();
        }
    }

    unsafe impl NSObjectProtocol for Controller {}

    unsafe impl NSWindowDelegate for Controller {
        /// 拖动（含程序性 setFrame）之后都存原始位置；越界交给启动时的 clamp 处理
        #[unsafe(method(windowDidMove:))]
        fn window_did_move(&self, _notification: &NSNotification) {
            self.persist();
        }
    }
}

impl Controller {
    pub fn new(
        mtm: MainThreadMarker,
        geometry: Geometry,
        file: Settings,
        chip: Chip,
        panel: Panel,
    ) -> Retained<Self> {
        let app = file.load();
        let sites_draft = [sites_text(&app.chat_sites), sites_text(&app.direct_sites)];
        let prompts = app.prompts.clone();
        // 编辑器打开时停在正在用的那条指令上
        let edit_index = app.active_prompt;
        let this = Self::alloc(mtm).set_ivars(Ivars {
            mtm,
            geometry,
            file,
            chip,
            panel,
            state: RefCell::new(State {
                app,
                pack: Pack::default(),
                payload: None,
                unread: None,
                last_clip: String::new(),
                // 不可能的 changeCount：第一次轮询就把剪贴板里已有的内容标成未读，与 Electron 一致
                last_change: -1,
                error: None,
                browsing: None,
                browser_refs: Vec::new(),
                settings_open: false,
                prompts,
                edit_index,
                sites_draft,
            }),
            weak: RefCell::new(None),
            anim: RefCell::new(None),
            anim_timer: RefCell::new(None),
        });
        unsafe { msg_send![super(this), init] }
    }

    /// 控件是先建后接线的：target 只能是本对象，所以放到构造之后
    pub fn start(controller: &Retained<Self>) {
        let myself: &Self = controller;
        *myself.ivars().weak.borrow_mut() = Some(Weak::from_retained(controller));
        let ivars = myself.ivars();
        let target: &AnyObject = myself.as_ref();
        for (button, action) in [
            (ivars.chip.dot.button(), sel!(expand:)),
            (ivars.chip.capture.button(), sel!(capture:)),
            (ivars.panel.collapse.as_ref(), sel!(collapse:)),
            (ivars.panel.capture.button(), sel!(capture:)),
            (ivars.panel.copy.button(), sel!(copy:)),
            (ivars.panel.quit.as_ref(), sel!(quit:)),
            (ivars.panel.browse.button(), sel!(browse:)),
            (ivars.panel.refresh.button(), sel!(refreshContext:)),
            (ivars.panel.settings_toggle.as_ref(), sel!(toggleSettings:)),
            (ivars.panel.pe_new.button(), sel!(newPrompt:)),
            (ivars.panel.pe_del.button(), sel!(delPrompt:)),
            (ivars.panel.set_save.button(), sel!(saveSettings:)),
        ] {
            views::wire(button, Some(target), action);
        }
        for label in [&ivars.panel.mode_read, &ivars.panel.mode_direct] {
            views::wire(label, Some(target), sel!(modeChanged:));
        }
        for pick in [&ivars.panel.agent_pick, &ivars.panel.turns_pick] {
            views::wire(pick, Some(target), sel!(refreshContext:));
        }
        views::wire(&ivars.panel.prompt_pick, Some(target), sel!(promptChanged:));
        views::wire(&ivars.panel.pe_pick, Some(target), sel!(peChanged:));
        myself.wire_sites();
        myself.wire_browser_rows();
        let delegate = Some(ProtocolObject::from_ref(myself));
        ivars.chip.window.setDelegate(delegate);
        ivars.panel.window.setDelegate(delegate);

        myself.sync_from_state();
        myself.reload_prompt_editor();
        myself.refresh();
        myself.notify();
        myself.watch_clipboard();
    }

    fn wire_sites(&self) {
        let ivars = self.ivars();
        let Some(target) = ivars.weak.borrow().clone().and_then(|w| w.load()) else {
            return;
        };
        let target: &AnyObject = target.as_ref();
        for site in ivars.panel.sites() {
            views::wire(site.button(), Some(target), sel!(openSite:));
        }
    }

    fn wire_browser_rows(&self) {
        let ivars = self.ivars();
        let Some(target) = ivars.weak.borrow().clone().and_then(|w| w.load()) else {
            return;
        };
        let target: &AnyObject = target.as_ref();
        for index in 0..ivars.panel.browser_row_count() {
            if let Some(button) = ivars.panel.browser_button(index) {
                views::wire(&button, Some(target), sel!(pickSession:));
            }
        }
    }

    // ---------- 模型 → 控件 ----------

    fn read_mode(&self) -> bool {
        self.ivars().state.borrow().app.with_context
    }

    fn current_sites(&self) -> Vec<SiteTarget> {
        let state = self.ivars().state.borrow();
        if state.app.with_context {
            state.app.chat_sites.clone()
        } else {
            state.app.direct_sites.clone()
        }
    }

    /// 把 settings / 草稿同步到控件上
    fn sync_from_state(&self) {
        let ivars = self.ivars();
        let state = ivars.state.borrow();
        let read = state.app.with_context;
        ivars.panel.set_read_mode(read);
        ivars.panel.set_turns(state.app.context_turns);
        ivars
            .panel
            .reload_prompts(&state.app.prompts, state.app.active_prompt);
        ivars.panel.reload_session_paths(&state.app.session_paths);
        ivars.panel.set_redact_on(state.app.redact_paths);
        ivars
            .panel
            .set_sites_text(&state.sites_draft[if read { 0 } else { 1 }]);
        ivars.panel.set_row(Row::Editor, state.settings_open);
        ivars
            .panel
            .set_copy_title(if read { COPY_READ } else { COPY_DIRECT });
    }

    fn reload_sites(&self) {
        let sites = self.current_sites();
        let ivars = self.ivars();
        ivars.panel.reload_sites(ivars.mtm, &sites);
        self.wire_sites();
        self.relayout();
    }

    fn refresh(&self) {
        let ivars = self.ivars();
        let state = ivars.state.borrow();
        let status = state.pack.status_line();
        let error = state.error.clone();
        drop(state);
        match error.as_deref() {
            Some(message) => {
                views::set_status(&ivars.chip.status, message, message, true);
                views::set_status(&ivars.panel.status, message, message, true);
            }
            None => {
                let (text, tip) = status;
                views::set_status(&ivars.chip.status, &text, &tip, false);
                views::set_status(&ivars.panel.status, &text, &tip, false);
            }
        }
        self.refresh_session_line();
        self.refresh_meta();
        // 状态行 / 字数行会按内容自己出现或消失，刷完一轮就得问一次高度
        self.relayout();
    }

    fn refresh_session_line(&self) {
        let ivars = self.ivars();
        let state = ivars.state.borrow();
        let (text, tip, error) = state.pack.session_line(state.app.with_context);
        drop(state);
        views::set_status(&ivars.panel.session_line, &text, &tip, error);
    }

    /// dropped 必须看得见：「组装后 N 字 · 已省略 M 类」
    fn refresh_meta(&self) {
        let ivars = self.ivars();
        let state = ivars.state.borrow();
        let (text, tip) = match &state.payload {
            None => (String::new(), String::new()),
            Some(payload) => payload.meta(),
        };
        drop(state);
        ivars.panel.set_pack_meta(&text, &tip);
    }

    fn notify(&self) {
        let ivars = self.ivars();
        let on = ivars.state.borrow().unread.is_some();
        views::set_dot(&ivars.chip.badge, on);
        views::set_dot(&ivars.panel.unread, on);
    }

    fn anchor(&self) -> (f64, f64) {
        let ivars = self.ivars();
        let visible = if ivars.chip.window.isVisible() {
            &ivars.chip.window
        } else {
            &ivars.panel.window
        };
        let rect = ivars.geometry.top_left(visible.frame());
        (rect.x, rect.y)
    }

    /// 内容变了就重量一次高度（Electron 的 autoHeight）。
    /// 高度差不为 0 时补一段过渡：面板不再「啪」地跳一下，而是长出来 / 收回去。
    /// 每帧都重画圆角遮罩（arrange_body 里做了），否则角会被拉成椭圆。
    fn relayout(&self) {
        let ivars = self.ivars();
        if !ivars.panel.window.isVisible() {
            return;
        }
        let anchor = self.anchor();
        let to = ivars.panel.wanted_body(&ivars.geometry, anchor);
        let from = ivars.panel.body();
        if Panel::reduce_motion() || (to - from).abs() < 2.0 {
            ivars.panel.arrange_to_content(&ivars.geometry, anchor);
            return;
        }
        // 已经在动就只改终点（下一拍自然折向新目标），不再另挂一条链：
        // 一次切模式会连着调 relayout 三遍（reload_sites → attach → refresh），
        // 每遍都挂链的话同一帧被驱动两次，实测 arrange 开销直接翻倍。
        *ivars.anim.borrow_mut() = Some(Anim {
            from,
            to,
            started: Instant::now(),
        });
        ivars.panel.set_animating(true);
        if ivars.anim_timer.borrow().is_none() {
            self.start_anim_timer();
        }
    }

    /// 一条 repeating 计时器跑完整段动画。
    /// 以前每帧重挂一个 one-shot，入列本身的 ~1.3ms 让帧距从 16.7ms 漂到 18ms
    /// （实测），对不上 60Hz 的 vsync —— 慢速位移看着就发黏。
    fn start_anim_timer(&self) {
        let Some(weak) = self.ivars().weak.borrow().clone() else {
            return;
        };
        let block = RcBlock::new(move |_timer: NonNull<NSTimer>| {
            if let Some(controller) = weak.load() {
                controller.anim_tick();
            }
        });
        let timer = unsafe {
            NSTimer::scheduledTimerWithTimeInterval_repeats_block(ANIM_TICK, true, &block)
        };
        *self.ivars().anim_timer.borrow_mut() = Some(timer);
    }

    fn stop_anim(&self) {
        let ivars = self.ivars();
        if let Some(timer) = ivars.anim_timer.borrow_mut().take() {
            timer.invalidate();
        }
        *ivars.anim.borrow_mut() = None;
        ivars.panel.set_animating(false);
    }

    /// 一拍：按已走的时间算缓出（decelerate，和入场同一条签名曲线），收拍落到真实内容高度
    fn anim_tick(&self) {
        let ivars = self.ivars();
        let Some(anim) = *ivars.anim.borrow() else {
            self.stop_anim();
            return;
        };
        let progress = anim.started.elapsed().as_secs_f64() / ANIM_SECONDS;
        if progress >= 1.0 {
            self.stop_anim();
            ivars
                .panel
                .arrange_to_content(&ivars.geometry, self.anchor());
            return;
        }
        let eased = 1.0 - (1.0 - progress).powi(3);
        let body = anim.from + (anim.to - anim.from) * eased;
        ivars
            .panel
            .arrange_body(&ivars.geometry, self.anchor(), body);
    }

    // ---------- 动作 ----------

    fn show(&self, expanded: bool) {
        let ivars = self.ivars();
        let anchor = self.anchor();
        if expanded {
            ivars.panel.arrange_to_content(&ivars.geometry, anchor);
        } else {
            let target = ivars.geometry.clamp(Rect {
                x: anchor.0,
                y: anchor.1,
                w: chip::WIDTH,
                h: chip::HEIGHT,
            });
            let origin = ivars
                .geometry
                .cocoa_origin(target.x, target.y, chip::HEIGHT);
            ivars.chip.window.setFrame_display(
                views::rect(origin.x, origin.y, chip::WIDTH, chip::HEIGHT),
                true,
            );
        }
        let (from, to): (&NSWindow, &NSWindow) = if expanded {
            (&ivars.chip.window, &ivars.panel.window)
        } else {
            (&ivars.panel.window, &ivars.chip.window)
        };
        // Electron 只在 win:shown 重放入场动画：已经开着的内容变化不能再闪一次
        let revealed = expanded && !to.isVisible();
        from.orderOut(None);
        to.orderFrontRegardless();
        if revealed {
            ivars.panel.play_enter();
        }
        // win:shown → 状态行重算 + 未读点重放
        self.refresh();
        self.notify();
        self.persist();
    }

    /// 取入选区：存 trim 后原文，无论成败都展开面板看结果
    fn capture_selection(&self) {
        let text = pasteboard::read_text().unwrap_or_default();
        let at = pasteboard::local_time();
        match capture::from_clipboard(&text, &at) {
            Ok(selection) => {
                {
                    let ivars = self.ivars();
                    let mut state = ivars.state.borrow_mut();
                    state.pack.set_selection(selection);
                    state.unread = None;
                    state.error = None;
                    state.browsing = None;
                }
                self.ivars().panel.set_row(Row::Browser, false);
                self.recompute_payload();
                self.notify();
                self.refresh();
                if self.read_mode() {
                    self.attach(None);
                }
            }
            Err(reason) => self.flash(&reason),
        }
        self.show(true);
    }

    /// 复制：直通是选区原文逐字节，会话解读是组装后的 Prompt
    fn copy_payload(&self) {
        let ivars = self.ivars();
        let prompt = ivars
            .state
            .borrow()
            .payload
            .as_ref()
            .map(|p| p.prompt.clone());
        let Some(prompt) = prompt else {
            self.flash("还没有取入选区");
            return;
        };
        if !pasteboard::write_text(&prompt) {
            self.flash("还没有取入选区");
            return;
        }
        {
            let mut state = ivars.state.borrow_mut();
            // 自己写回的东西不能反过来点亮未读点（Electron 在 pack:copy 里同步 lastClip）
            state.last_clip = prompt;
            state.last_change = pasteboard::change_count();
        }
        ivars.panel.set_copy_title(COPIED);
        ivars.panel.set_copy_done(true);
        self.after(COPIED_HOLD, After::ResetCopyLabel);
        self.refresh_meta();
    }

    fn set_mode(&self, read: bool) {
        if self.read_mode() == read {
            return;
        }
        {
            let ivars = self.ivars();
            let mut state = ivars.state.borrow_mut();
            let outgoing = usize::from(!state.app.with_context);
            state.sites_draft[outgoing] = ivars.panel.sites_text();
            state.app.with_context = read;
            if !read {
                state.browsing = None;
                state.pack.clear_context();
                ivars.panel.set_row(Row::Browser, false);
            }
        }
        self.persist_settings();
        self.sync_from_state();
        self.reload_sites();
        if read {
            self.attach(None);
        } else {
            // 直通的 Payload 就是选区原文，字数行和「复制选区原文」都要它（Electron 同款）
            self.recompute_payload();
            // refresh() 自己会 relayout：这里再排一次会让高度动画从头重启
            self.refresh();
        }
    }

    /// 显式触发才会跑（可能要几百毫秒）
    fn attach(&self, explicit: Option<&SessionRef>) {
        let ivars = self.ivars();
        if !self.read_mode() {
            return;
        }
        let turns = ivars.panel.turns();
        let picked = explicit
            .cloned()
            .or_else(|| ivars.state.borrow().browsing.clone());
        let (agent, file, session) = match picked.as_ref() {
            Some(reference) => (
                reference.agent.clone(),
                Some(reference.file_path.clone()),
                reference.session_id.clone(),
            ),
            None => (ivars.panel.agent_token(), None, None),
        };
        ivars.panel.set_ctx_status("正在填充上下文…", false);
        ivars.panel.redraw();
        let settings = ivars.state.borrow().app.clone();
        ivars.state.borrow_mut().pack.attach(
            &settings,
            &agent,
            turns,
            file.as_deref(),
            session.as_deref(),
        );
        self.recompute_payload();
        // 失败原因只留一处：会话行本来就写着「上下文：<原因>」，
        // 状态行再抄一遍就是两行同义的橙色（Electron 那侧是历史遗留，native 不跟）
        ivars.panel.set_ctx_status("", false);
        self.refresh_session_line();
        self.relayout();
    }

    fn recompute_payload(&self) {
        let ivars = self.ivars();
        let settings = ivars.state.borrow().app.clone();
        let payload = ivars.state.borrow().pack.payload(&settings);
        ivars.state.borrow_mut().payload = payload;
        self.refresh_meta();
    }

    fn toggle_browser(&self) {
        let ivars = self.ivars();
        if ivars.panel.row_on(Row::Browser) {
            ivars.panel.set_row(Row::Browser, false);
            self.relayout();
            return;
        }
        ivars.panel.set_row(Row::Browser, true);
        ivars.panel.show_browser(ivars.mtm, &[], None);
        ivars.panel.set_browser_note(ivars.mtm, "正在发现会话…");
        self.relayout();
        ivars.panel.redraw();

        let settings = ivars.state.borrow().app.clone();
        let refs = context::browse(&settings, context::BROWSE_LIMIT);
        // 手动挑过的那条要在列表里打勾：.db 文件里几十个会话共用同一个路径，得连会话 id 一起比
        let selected = ivars.state.borrow().browsing.as_ref().and_then(|b| {
            refs.iter()
                .find(|r| r.file_path == b.file_path && r.session_id == b.session_id)
                .cloned()
        });
        ivars.state.borrow_mut().browser_refs = refs.clone();
        ivars
            .panel
            .show_browser(ivars.mtm, &refs, selected.as_ref());
        if refs.is_empty() {
            ivars
                .panel
                .set_browser_note(ivars.mtm, "未发现任何可解析的会话");
        }
        self.wire_browser_rows();
        self.relayout();
    }

    // ---------- 设置编辑器 ----------

    fn reload_prompt_editor(&self) {
        let ivars = self.ivars();
        let state = ivars.state.borrow();
        ivars
            .panel
            .reload_editor_prompts(&state.prompts, state.edit_index);
        let template = state
            .prompts
            .get(state.edit_index)
            .map(|p| p.template.clone())
            .unwrap_or_default();
        let (index, total) = (state.edit_index + 1, state.prompts.len());
        drop(state);
        ivars.panel.set_template_text(&template);
        ivars.panel.prompt_count(index, total);
    }

    /// 把编辑框里的改动写回草稿（切指令、保存前都要先做）
    fn commit_template(&self) {
        let text = self.ivars().panel.template_text();
        let mut state = self.ivars().state.borrow_mut();
        let index = state.edit_index;
        if let Some(current) = state.prompts.get_mut(index) {
            current.template = text.trim().to_string();
        }
    }

    fn save_settings(&self) {
        self.commit_template();
        let ivars = self.ivars();
        let panel_text = (
            ivars.panel.session_paths_text(),
            ivars.panel.sites_text(),
            ivars.panel.redact_on(),
            ivars.panel.turns(),
        );
        let mut state = ivars.state.borrow_mut();
        let prompts: Vec<PromptTemplate> = state
            .prompts
            .iter()
            .filter(|p| !p.name.is_empty() && !p.template.is_empty())
            .cloned()
            .collect();
        let (paths, bad_paths) = parse_session_paths(&panel_text.0);
        let (sites, bad_sites) = parse_sites(&panel_text.1);
        let read = state.app.with_context;

        if !prompts.is_empty() {
            state.app.prompts = prompts;
            state.app.active_prompt = state.app.active_prompt.min(state.app.prompts.len() - 1);
        }
        state.app.session_paths = paths;
        state.app.redact_paths = panel_text.2;
        state.app.context_turns = panel_text.3;
        let slot = usize::from(!read);
        // 草稿永远镜像编辑框：无效行得留在原地让用户改，不能被回写后的旧值盖掉
        state.sites_draft[slot] = panel_text.1.clone();
        if !sites.is_empty() {
            if read {
                state.app.chat_sites = sites;
            } else {
                state.app.direct_sites = sites;
            }
        }
        state.prompts = state.app.prompts.clone();
        state.edit_index = state.edit_index.min(state.prompts.len().saturating_sub(1));
        let app = state.app.clone();
        drop(state);

        if let Err(e) = ivars.file.save(&app) {
            self.flash(&format!("保存设置失败：{e}"));
            return;
        }
        let mut warn = Vec::new();
        if !bad_sites.is_empty() {
            warn.push(format!("站点第 {} 行", joined(&bad_sites)));
        }
        if !bad_paths.is_empty() {
            warn.push(format!("会话路径第 {} 行", joined(&bad_paths)));
        }
        if warn.is_empty() {
            self.flash("已保存");
        } else {
            self.flash(&format!(
                "无效行（站点需 名称|http(s)://URL，路径需 {}|路径）：{}，未生效",
                AGENT_TOKENS.join("/"),
                warn.join("，")
            ));
        }
        self.sync_from_state();
        self.reload_prompt_editor();
        self.reload_sites();
        self.recompute_payload();
    }

    fn persist_settings(&self) {
        let ivars = self.ivars();
        let app = ivars.state.borrow().app.clone();
        if let Err(e) = ivars.file.save(&app) {
            eprintln!("select-assist-native: 没能写回 settings.json: {e}");
        }
    }

    /// 短暂提示：状态行显示几秒后回到模型
    fn flash(&self, message: &str) {
        let ivars = self.ivars();
        let is_error = message != "已保存";
        ivars.state.borrow_mut().error = is_error.then(|| message.to_string());
        ivars.panel.set_ctx_status(message, is_error);
        // 提示会自己收回（Electron 的 flashStatus 对非错误 4s 后清空）。
        // 状态行现在按内容占位，不收回就是永久多出一行。
        self.after(
            if is_error { ERROR_HOLD } else { FLASH_HOLD },
            After::ClearFlash,
        );
        self.refresh();
    }

    // ---------- 剪贴板轮询 ----------

    fn watch_clipboard(&self) {
        let Some(weak) = self.ivars().weak.borrow().clone() else {
            return;
        };
        let block = RcBlock::new(move |_timer: NonNull<NSTimer>| {
            if let Some(controller) = weak.load() {
                controller.poll();
            }
        });
        unsafe {
            NSTimer::scheduledTimerWithTimeInterval_repeats_block(POLL_SECONDS, true, &block);
        }
    }

    /// 轮询只负责点亮红点，绝不自动打开面板、自动取入
    fn poll(&self) {
        let ivars = self.ivars();
        let count = pasteboard::change_count();
        if count == ivars.state.borrow().last_change {
            return;
        }
        ivars.state.borrow_mut().last_change = count;
        let Some(text) = pasteboard::read_text() else {
            return;
        };
        if text.is_empty() || text == ivars.state.borrow().last_clip {
            return;
        }
        let mut state = ivars.state.borrow_mut();
        state.last_clip = text.clone();
        state.unread = Some(capture::note_from(&text));
        drop(state);
        self.notify();
    }

    fn after(&self, seconds: f64, what: After) {
        let Some(weak) = self.ivars().weak.borrow().clone() else {
            return;
        };
        let block = RcBlock::new(move |_timer: NonNull<NSTimer>| {
            if let Some(controller) = weak.load() {
                controller.apply(what);
            }
        });
        unsafe {
            NSTimer::scheduledTimerWithTimeInterval_repeats_block(seconds, false, &block);
        }
    }

    fn apply(&self, what: After) {
        match what {
            After::ClearFlash => {
                let ivars = self.ivars();
                ivars.state.borrow_mut().error = None;
                ivars.panel.set_ctx_status("", false);
                self.refresh();
            }
            After::ResetCopyLabel => {
                let read = self.ivars().state.borrow().app.with_context;
                let panel = &self.ivars().panel;
                panel.set_copy_title(if read { COPY_READ } else { COPY_DIRECT });
                panel.set_copy_done(false);
            }
        }
    }

    /// 写的是当前可见那个窗口的左上角坐标 —— 两窗共用同一个锚点，谁可见存谁
    fn persist(&self) {
        let ivars = self.ivars();
        if ivars.panel.is_animating() {
            return;
        }
        let visible = if ivars.chip.window.isVisible() {
            &ivars.chip.window
        } else {
            &ivars.panel.window
        };
        let anchor = ivars.geometry.top_left(visible.frame());
        if let Err(e) = ivars.file.patch_position(anchor.x, anchor.y) {
            eprintln!("select-assist-native: 没能写回 settings.json: {e}");
        }
    }
}

/// 「第 2、3 行」这种提示
fn joined(rows: &[usize]) -> String {
    rows.iter()
        .map(|r| r.to_string())
        .collect::<Vec<_>>()
        .join("、")
}

fn sites_text(sites: &[SiteTarget]) -> String {
    sites
        .iter()
        .map(|s| format!("{}|{}", s.name, s.url))
        .collect::<Vec<_>>()
        .join("\n")
}

/// 每行 `第一段|第二段`：跳过空行，交出 (1 起的行号, 两段)；没有竖线时第二段是空串
fn split_pairs(text: &str) -> Vec<(usize, String, String)> {
    text.lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(index, line)| {
            let parts: Vec<&str> = line.split('|').collect();
            (
                index + 1,
                parts[0].trim().to_string(),
                parts
                    .get(1)
                    .map(|p| p.trim())
                    .unwrap_or_default()
                    .to_string(),
            )
        })
        .collect()
}

/// 每行 `agent|路径`，坏行记下来但不打断其它行
fn parse_session_paths(text: &str) -> (Vec<SessionPath>, Vec<usize>) {
    let mut out = Vec::new();
    let mut bad = Vec::new();
    for (line, agent, path) in split_pairs(text) {
        let agent = agent.to_lowercase();
        if AGENT_TOKENS.contains(&agent.as_str()) && !path.is_empty() {
            out.push(SessionPath { agent, path });
        } else {
            bad.push(line);
        }
    }
    (out, bad)
}

/// 每行 `名称|http(s)://URL`
fn parse_sites(text: &str) -> (Vec<SiteTarget>, Vec<usize>) {
    let mut out = Vec::new();
    let mut bad = Vec::new();
    for (line, name, url) in split_pairs(text) {
        if !name.is_empty() && has_http_scheme(&url) {
            out.push(SiteTarget { name, url });
        } else {
            bad.push(line);
        }
    }
    (out, bad)
}

/// 对应 renderer.js 的 `/^https?:\/\//i`：大写协议同样算数
fn has_http_scheme(url: &str) -> bool {
    let lowered = url.to_ascii_lowercase();
    lowered.starts_with("http://") || lowered.starts_with("https://")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_path_lines_parse_like_the_renderer() {
        let (paths, bad) = parse_session_paths(
            "qoder|/tmp/a\nproject|/tmp/b\nbogus|/x\n\nAUTO|/tmp/c\nCodex|/tmp/d\n没有竖线",
        );
        assert_eq!(paths.len(), 4);
        assert_eq!(paths[0].agent, "qoder");
        assert_eq!(paths[1].agent, "project");
        assert_eq!(paths[2].agent, "auto", "agent 段大小写不敏感");
        assert_eq!(paths[3].agent, "codex");
        assert_eq!(bad, vec![3, 7], "空行跳过，坏行报原始行号");
    }

    #[test]
    fn site_lines_need_a_name_and_an_http_url() {
        let (sites, bad) = parse_sites("Google|https://www.google.com/\n坏行 no pipe\nfile|file:///etc/hosts\nDeepL|http://www.deepl.com");
        assert_eq!(sites.len(), 2);
        assert_eq!(bad, vec![2, 3]);
    }

    #[test]
    fn an_uppercase_scheme_is_still_a_site() {
        // renderer.js 用 /^https?:\/\//i，大写不能算坏行
        let (sites, bad) = parse_sites(
            "Google|HTTPS://www.google.com/\nBing|HTTP://www.bing.com/\n|https://缺名字.com/",
        );
        assert_eq!(bad, vec![3]);
        assert_eq!(
            sites[0].url, "HTTPS://www.google.com/",
            "URL 原样存，不改写大小写"
        );
        assert_eq!(sites[1].name, "Bing");
    }
}
