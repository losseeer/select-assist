//! 实验：在 M0 那个 400×44 窗口里挂一个 WebView2，加载 Electron 版现成的 chip 页面。
//! 只为拿一个数字——「复用前端」这条路的真实开销（就绪耗时 + 进程内存），好跟手画
//! Direct2D 那条路比。SA_WEBVIEW=1 才启用，M0 的行为不受影响。

use std::cell::RefCell;
use std::time::Instant;

use webview2_com::{
    CreateCoreWebView2ControllerCompletedHandler, CreateCoreWebView2EnvironmentCompletedHandler,
};
use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::{HWND, RECT};
use webview2_com::Microsoft::Web::WebView2::Win32::{
    CreateCoreWebView2EnvironmentWithOptions, ICoreWebView2Controller, ICoreWebView2Environment,
};

thread_local! {
    // COM 回调只会在本线程（STA）上跑，所以用 thread_local 而不是 channel：
    // 那些接口不是 Send，塞进 channel 反而过不了编译
    static STARTED: RefCell<Option<Instant>> = const { RefCell::new(None) };
    static ENV: RefCell<Option<ICoreWebView2Environment>> = const { RefCell::new(None) };
    static CTRL: RefCell<Option<ICoreWebView2Controller>> = const { RefCell::new(None) };
    static PENDING_URL: RefCell<Option<String>> = const { RefCell::new(None) };
}

pub fn start(hwnd: HWND, user_data: &str, url: &str) {
    STARTED.with(|t| *t.borrow_mut() = Some(Instant::now()));
    PENDING_URL.with(|u| *u.borrow_mut() = Some(url.to_string()));

    let handler = CreateCoreWebView2EnvironmentCompletedHandler::create(Box::new(|result, env| {
        if let Err(e) = result {
            println!("webview env failed: {e}");
            return Ok(());
        }
        if let Some(env) = env {
            ENV.with(|e| *e.borrow_mut() = Some(env.clone()));
        }
        Ok(())
    }));
    let wide: Vec<u16> = user_data.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        CreateCoreWebView2EnvironmentWithOptions(
            PCWSTR::null(),
            PCWSTR(wide.as_ptr()),
            None,
            &handler,
        )
        .expect("CreateCoreWebView2EnvironmentWithOptions")
    };
    let _ = hwnd;
}

/// 每次心跳调用；COM 回调在本线程消息队列里送达，所以必须在消息循环里推进
pub fn tick(hwnd: HWND) {
    if let (Some(env), None) = (
        ENV.with(|e| e.borrow().clone()),
        CTRL.with(|c| c.borrow().clone()),
    ) {
        let handler = CreateCoreWebView2ControllerCompletedHandler::create(Box::new(|result, ctrl| {
            if let Err(e) = result {
                println!("webview controller failed: {e}");
                return Ok(());
            }
            if let Some(ctrl) = ctrl {
                CTRL.with(|c| *c.borrow_mut() = Some(ctrl.clone()));
            }
            Ok(())
        }));
        unsafe {
            env.CreateCoreWebView2Controller(hwnd, &handler)
                .expect("CreateCoreWebView2Controller")
        };
        ENV.with(|e| *e.borrow_mut() = None);
    }

    let Some(ctrl) = CTRL.with(|c| c.borrow().clone()) else {
        return;
    };
    let Some(url) = PENDING_URL.with(|u| u.borrow().clone()) else {
        return;
    };
    unsafe {
        // 这一版绑定里 SetBounds 收的是 RECT
        ctrl.SetBounds(RECT {
            left: 0,
            top: 0,
            right: 400,
            bottom: 44,
        })
        .expect("SetBounds");
        // 控制器默认不可见，runtime 会因此不拉起 browser 进程
        ctrl.SetIsVisible(true).expect("SetIsVisible");
        let webview = ctrl.CoreWebView2().expect("CoreWebView2");
        webview
            .Navigate(&HSTRING::from(url))
            .expect("Navigate(url)");
        println!(
            "webview 就绪用时 {}ms",
            STARTED.with(|t| t.borrow().map(|i| i.elapsed().as_millis()).unwrap_or(0))
        );
    }
    CTRL.with(|c| *c.borrow_mut() = None);
    PENDING_URL.with(|u| u.borrow_mut().take());
}
