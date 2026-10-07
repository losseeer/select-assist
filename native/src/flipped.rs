//! 正文用的 flipped 容器：y 向下增长，排版与 CSS 一致，滚动也从顶部开始。

use objc2::rc::Retained;
use objc2::{define_class, msg_send, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::NSView;
use objc2_foundation::{NSObjectProtocol, NSRect};

define_class! {
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    pub struct FlippedView;

    impl FlippedView {
        #[unsafe(method(isFlipped))]
        fn flipped(&self) -> bool {
            true
        }
    }

    unsafe impl NSObjectProtocol for FlippedView {}
}

pub fn flipped_view(mtm: MainThreadMarker, frame: NSRect) -> Retained<FlippedView> {
    unsafe { msg_send![FlippedView::alloc(mtm), initWithFrame: frame] }
}
