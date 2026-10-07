//! 正文用的 flipped 容器：y 向下增长，排版与 CSS 一致，滚动也从顶部开始。

use objc2::rc::Retained;
use objc2::{define_class, msg_send, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSClipView, NSView};
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

define_class! {
    /// 视口也要 flipped：NSClipView 默认不翻转，flipped 的 documentView 比视口矮时
    /// 会被它按「底边对齐」摆放 —— 实测直通模式正文只有 182pt、视口 240pt，
    /// 模式行与动作行之间凭空多出 58pt 空洞；设置编辑器更高时顶部的模式行直接被推出可视区。
    /// Electron 那边是文档流从顶部开始，所以这里补一个 flipped 的 clip view 对齐行为。
    #[unsafe(super(NSClipView))]
    #[thread_kind = MainThreadOnly]
    pub struct FlippedClipView;

    impl FlippedClipView {
        #[unsafe(method(isFlipped))]
        fn flipped(&self) -> bool {
            true
        }
    }

    unsafe impl NSObjectProtocol for FlippedClipView {}
}

pub fn flipped_clip(mtm: MainThreadMarker, frame: NSRect) -> Retained<FlippedClipView> {
    unsafe { msg_send![FlippedClipView::alloc(mtm), initWithFrame: frame] }
}
