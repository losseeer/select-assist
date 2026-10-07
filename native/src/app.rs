//! 双窗调度：expand/collapse 是同 origin 的高度切换，拖动后写回 settings.json，
//! 启动时读回并 clampToDisplay。对应 Electron main/index.ts 的 createWindows/expand/collapse。

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{define_class, msg_send, sel, DeclaredClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSApplication, NSWindowDelegate};
use objc2_foundation::{NSNotification, NSObject, NSObjectProtocol, NSRect, NSSize};

use crate::chip::Chip;
use crate::geo::{Geometry, Rect};
use crate::panel::Panel;
use crate::settings::Settings;

pub struct Ivars {
    mtm: MainThreadMarker,
    geometry: Geometry,
    settings: Settings,
    chip: Chip,
    panel: Panel,
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
    ) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(Ivars {
            mtm,
            geometry,
            settings,
            chip,
            panel,
        });
        unsafe { msg_send![super(this), init] }
    }

    /// 控件是先建后接线的：target 只能是本对象，所以放到构造之后
    pub fn start(&self) {
        let ivars = self.ivars();
        let target: &AnyObject = self.as_ref();
        unsafe {
            ivars.chip.dot.setTarget(Some(target));
            ivars.chip.dot.setAction(Some(sel!(expand:)));
            ivars.panel.collapse.setTarget(Some(target));
            ivars.panel.collapse.setAction(Some(sel!(collapse:)));
            ivars.panel.quit.setTarget(Some(target));
            ivars.panel.quit.setAction(Some(sel!(quit:)));
        }
        let delegate = Some(ProtocolObject::from_ref(self));
        ivars.chip.window.setDelegate(delegate);
        ivars.panel.window.setDelegate(delegate);
    }

    /// 两窗同左上角 origin，只有高度在变；宽度相同所以 x 不动
    fn show(&self, expanded: bool) {
        let ivars = self.ivars();
        let (from, to, width, height) = if expanded {
            (
                &ivars.chip.window,
                &ivars.panel.window,
                crate::panel::WIDTH,
                ivars.panel.height(),
            )
        } else {
            (
                &ivars.panel.window,
                &ivars.chip.window,
                crate::chip::WIDTH,
                crate::chip::HEIGHT,
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
