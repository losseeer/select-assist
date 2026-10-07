//! 双窗调度 + M2 直通闭环。
//! 轮询只点亮未读点；「取入选区」把剪贴板原文（trim 后逐字节）存下来，
//! 「复制选区原文」再原样写回去；站点按钮走 NSWorkspace。对应 main/index.ts 与 capture.ts。

use std::cell::{Cell, RefCell};
use std::ptr::NonNull;

use block2::RcBlock;
use objc2::rc::{Retained, Weak};
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{define_class, msg_send, sel, DeclaredClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSApplication, NSButton, NSWindowDelegate};
use objc2_foundation::{NSNotification, NSObject, NSObjectProtocol, NSRect, NSSize, NSTimer};

use crate::capture::{self, ClipNote, Selection};
use crate::chip::{self, Chip};
use crate::geo::{Geometry, Rect};
use crate::panel::{self, Panel};
use crate::pasteboard;
use crate::settings::{Settings, SiteTarget};
use crate::views;

/// Electron 的轮询周期
const POLL_SECONDS: f64 = 0.8;
/// 错误文案停留（chip 的 setTimeout(refreshChipStatus, 3000)）
const ERROR_HOLD: f64 = 3.0;
/// 「已复制 ✓」的反馈时长
const COPIED_HOLD: f64 = 1.2;
const COPY_TITLE: &str = "复制选区原文";
const COPIED_TITLE: &str = "已复制 ✓";
const NO_SELECTION_TO_COPY: &str = "还没有取入选区";

/// 延时任务只有这两种，用枚举而不是闭包，免得往 ivars 里塞 boxed fn
#[derive(Clone, Copy)]
enum After {
    ClearError,
    ResetCopyLabel,
}

pub struct Ivars {
    mtm: MainThreadMarker,
    geometry: Geometry,
    settings: Settings,
    chip: Chip,
    panel: Panel,
    sites: Vec<SiteTarget>,
    /// 轮询基准：自己写回剪贴板时必须同步它，否则自我复制会点亮红点
    last_clip: RefCell<String>,
    last_change: Cell<isize>,
    unread: RefCell<Option<ClipNote>>,
    selection: RefCell<Option<Selection>>,
    error: RefCell<Option<String>>,
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
            self.copy_selection();
        }

        #[unsafe(method(openSite:))]
        fn on_open_site(&self, sender: Option<&AnyObject>) {
            let Some(button) = sender.and_then(|s| s.downcast_ref::<NSButton>()) else {
                return;
            };
            let Some(site) = self.ivars().sites.get(button.tag() as usize) else {
                return;
            };
            if !pasteboard::open_site(&site.url) {
                eprintln!("select-assist-native: 打不开站点「{}」", site.name);
            }
        }

        #[unsafe(method(quit:))]
        fn on_quit(&self, _sender: Option<&AnyObject>) {
            NSApplication::sharedApplication(self.ivars().mtm).terminate(None);
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
        settings: Settings,
        chip: Chip,
        panel: Panel,
        sites: Vec<SiteTarget>,
    ) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(Ivars {
            mtm,
            geometry,
            settings,
            chip,
            panel,
            sites,
            // 空 lastClip + 不可能的 changeCount：第一次轮询就会把剪贴板里已有的内容标成未读，
            // 与 Electron 启动 800ms 后的表现一致
            last_clip: RefCell::new(String::new()),
            last_change: Cell::new(-1),
            unread: RefCell::new(None),
            selection: RefCell::new(None),
            error: RefCell::new(None),
            weak: RefCell::new(None),
        });
        unsafe { msg_send![super(this), init] }
    }

    /// 控件是先建后接线的：target 只能是本对象，所以放到构造之后
    /// target 只能指向自己，所以入口收的是 `Retained`（用来造 weak 引用）
    pub fn start(controller: &Retained<Self>) {
        let myself: &Self = controller;
        *myself.ivars().weak.borrow_mut() = Some(Weak::from_retained(controller));
        let ivars = myself.ivars();
        let target: &AnyObject = myself.as_ref();
        let actions = [
            (&ivars.chip.dot, sel!(expand:)),
            (&ivars.chip.capture, sel!(capture:)),
            (&ivars.panel.collapse, sel!(collapse:)),
            (&ivars.panel.capture, sel!(capture:)),
            (&ivars.panel.copy, sel!(copy:)),
            (&ivars.panel.quit, sel!(quit:)),
        ];
        for (button, action) in actions {
            unsafe {
                button.setTarget(Some(target));
                button.setAction(Some(action));
            }
        }
        for button in &ivars.panel.sites {
            unsafe {
                button.setTarget(Some(target));
                button.setAction(Some(sel!(openSite:)));
            }
        }
        let delegate = Some(ProtocolObject::from_ref(myself));
        ivars.chip.window.setDelegate(delegate);
        ivars.panel.window.setDelegate(delegate);

        myself.refresh();
        myself.notify();
        myself.watch_clipboard();
    }

    /// 轮询只负责点亮红点，绝不自动打开面板、自动取入（照搬 startClipboardWatch）
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

    fn poll(&self) {
        let ivars = self.ivars();
        let count = pasteboard::change_count();
        if count == ivars.last_change.get() {
            return;
        }
        ivars.last_change.set(count);
        let Some(text) = pasteboard::read_text() else {
            return;
        };
        if text.is_empty() || text == *ivars.last_clip.borrow() {
            return;
        }
        *ivars.last_clip.borrow_mut() = text.clone();
        *ivars.unread.borrow_mut() = Some(capture::note_from(&text));
        self.notify();
    }

    /// chip 的小红点与面板「取入选区」上的未读点同步亮灭
    fn notify(&self) {
        let ivars = self.ivars();
        let on = ivars.unread.borrow().is_some();
        views::set_dot(&ivars.chip.badge, on);
        views::set_dot(&ivars.panel.unread, on);
    }

    /// 状态行一律从模型算：错误优先，否则是选区首行 / 「还没有选区」
    fn refresh(&self) {
        let ivars = self.ivars();
        let selection = ivars.selection.borrow();
        let error = ivars.error.borrow();
        match error.as_deref() {
            Some(message) => {
                views::set_status(&ivars.chip.status, message, message, true);
                views::set_status(&ivars.panel.status, message, message, true);
            }
            None => {
                let current = selection.as_ref();
                let (text, tip) = (capture::status_text(current), capture::status_tip(current));
                views::set_status(&ivars.chip.status, &text, &tip, false);
                views::set_status(&ivars.panel.status, &text, &tip, false);
            }
        }
        let meta = capture::meta_text(selection.as_ref());
        views::set_status(&ivars.panel.meta, &meta, "", false);
    }

    fn flash(&self, message: &str) {
        *self.ivars().error.borrow_mut() = Some(message.to_string());
        self.after(ERROR_HOLD, After::ClearError);
        self.refresh();
    }

    /// 取入选区：trim 后的原文进模型，空白剪贴板算失败；无论成败都展开面板看结果
    fn capture_selection(&self) {
        let ivars = self.ivars();
        let text = pasteboard::read_text().unwrap_or_default();
        match capture::from_clipboard(&text, &pasteboard::local_time()) {
            Ok(selection) => {
                *ivars.selection.borrow_mut() = Some(selection);
                *ivars.unread.borrow_mut() = None;
                *ivars.error.borrow_mut() = None;
                self.notify();
                self.refresh();
            }
            Err(reason) => self.flash(&reason),
        }
        self.show(true);
    }

    /// 复制选区原文：逐字节写回，不套模板也不脱敏
    fn copy_selection(&self) {
        let ivars = self.ivars();
        let Some(text) = ivars.selection.borrow().as_ref().map(|s| s.text.clone()) else {
            self.flash(NO_SELECTION_TO_COPY);
            return;
        };
        if !pasteboard::write_text(&text) {
            self.flash(NO_SELECTION_TO_COPY);
            return;
        }
        // 自己写回的东西不能反过来点亮未读点（Electron 在 pack:copy 里同步 lastClip）
        *ivars.last_clip.borrow_mut() = text;
        ivars.last_change.set(pasteboard::change_count());
        views::set_title(&ivars.panel.copy, COPIED_TITLE, None);
        self.after(COPIED_HOLD, After::ResetCopyLabel);
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
            After::ClearError => {
                *self.ivars().error.borrow_mut() = None;
                self.refresh();
            }
            After::ResetCopyLabel => views::set_title(&self.ivars().panel.copy, COPY_TITLE, None),
        }
    }

    /// 两窗同左上角 origin，只有高度在变；宽度相同所以 x 不动
    fn show(&self, expanded: bool) {
        let ivars = self.ivars();
        let (from, to, width, height) = if expanded {
            (
                &ivars.chip.window,
                &ivars.panel.window,
                panel::WIDTH,
                ivars.panel.height(),
            )
        } else {
            (
                &ivars.panel.window,
                &ivars.chip.window,
                chip::WIDTH,
                chip::HEIGHT,
            )
        };

        let anchor = ivars.geometry.top_left(from.frame());
        let target = ivars.geometry.clamp(Rect {
            x: anchor.x,
            y: anchor.y,
            w: width,
            h: height,
        });
        if expanded {
            ivars.panel.arrange(height);
        }
        let origin = ivars.geometry.cocoa_origin(target.x, target.y, height);
        to.setFrame_display(NSRect::new(origin, NSSize::new(width, height)), true);
        from.orderOut(None);
        to.orderFrontRegardless();
        // win:shown → 状态行重算 + 未读点重放
        self.refresh();
        self.notify();
        self.persist();
    }

    /// 写的是当前可见那个窗口的左上角坐标 —— 两窗共用同一个锚点，谁可见存谁
    fn persist(&self) {
        let ivars = self.ivars();
        let visible = if ivars.chip.window.isVisible() {
            &ivars.chip.window
        } else {
            &ivars.panel.window
        };
        let anchor = ivars.geometry.top_left(visible.frame());
        if let Err(e) = ivars.settings.patch_position(anchor.x, anchor.y) {
            eprintln!("select-assist-native: 没能写回 settings.json: {e}");
        }
    }
}
