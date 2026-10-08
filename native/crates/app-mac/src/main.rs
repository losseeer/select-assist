mod app;
mod capture;
mod chip;
mod context;
mod flipped;
mod geo;
mod panel;
mod pasteboard;
mod settings;
mod views;

use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy, NSRunningApplication};
use objc2_foundation::NSBundle;

use crate::geo::{Geometry, Rect};

fn main() {
    let mtm = MainThreadMarker::new().expect("AppKit needs the main thread");
    let app = NSApplication::sharedApplication(mtm);
    // 与 Electron 版的常驻圆点一样：不占 Dock、不进 ⌘Tab
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    if another_instance_is_running() {
        eprintln!("select-assist-native: 已经有一个在跑了，这个实例退出");
        return;
    }

    let geometry = Geometry::current(mtm);
    let settings = settings::Settings::shared();
    let anchor = start_anchor(&geometry, settings.position());

    let app_settings = settings.load();
    let chip = chip::Chip::create(mtm, &geometry, anchor);
    // 站点跟着模式走：会话解读开聊天站，直通开词典站
    let sites = if app_settings.with_context {
        &app_settings.chat_sites
    } else {
        &app_settings.direct_sites
    };
    let panel = panel::Panel::create(
        mtm,
        &geometry,
        anchor,
        sites,
        &app_settings.prompts,
        &app_settings.session_paths,
        app_settings.redact_paths,
        app_settings.context_turns,
        app_settings.with_context,
        app_settings.active_prompt,
    );
    let controller = app::Controller::new(mtm, geometry, settings, chip, panel);
    app::Controller::start(&controller);
    app.run();
}

/// §3：同一 bundle id 只留一个实例。两版共用一份 settings.json，两个 chip 各自
/// read-modify-write 会互相盖掉位置与设置。裸二进制（`cargo run`、target/debug/…）
/// 没有 bundle id，跳过这道检查，沙箱并排调试照旧。
fn another_instance_is_running() -> bool {
    let Some(id) = NSBundle::mainBundle().bundleIdentifier() else {
        return false;
    };
    let mine = std::process::id() as i32;
    let same = NSRunningApplication::runningApplicationsWithBundleIdentifier(&id);
    (0..same.count()).any(|i| same.objectAtIndex(i).processIdentifier() != mine)
}

/// 启动锚点（窗口左上角的全局坐标）：settings 里存的优先，否则主屏 workArea 右上角内缩 16/60；
/// 断掉一块显示器之后靠 clamp 把窗口拉回来。两窗共用这一个锚点，各自按自己的高度换算。
fn start_anchor(geometry: &Geometry, saved: Option<(f64, f64)>) -> (f64, f64) {
    let work = geometry.displays.first().map_or(
        Rect {
            x: 0.0,
            y: 0.0,
            w: chip::WIDTH,
            h: chip::HEIGHT,
        },
        |display| display.work,
    );
    let wanted = saved.map_or(
        Rect {
            x: work.x + work.w - chip::WIDTH - chip::MARGIN_RIGHT,
            y: work.y + chip::MARGIN_TOP,
            w: chip::WIDTH,
            h: chip::HEIGHT,
        },
        |(x, y)| Rect {
            x,
            y,
            w: chip::WIDTH,
            h: chip::HEIGHT,
        },
    );
    let clamped = geometry.clamp(wanted);
    (clamped.x, clamped.y)
}
