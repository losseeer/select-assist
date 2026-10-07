mod app;
mod chip;
mod geo;
mod panel;
mod settings;
mod views;

use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};

use crate::geo::{Geometry, Rect};

fn main() {
    let mtm = MainThreadMarker::new().expect("AppKit needs the main thread");
    let app = NSApplication::sharedApplication(mtm);
    // 与 Electron 版的常驻圆点一样：不占 Dock、不进 ⌘Tab
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

    let geometry = Geometry::current(mtm);
    let settings = settings::Settings::shared();
    let anchor = start_anchor(&geometry, settings.position());

    let chip = chip::Chip::create(mtm, &geometry, anchor);
    let panel = panel::Panel::create(mtm, &geometry, anchor);
    let controller = app::Controller::new(mtm, geometry, settings, chip, panel);
    controller.start();
    app.run();
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
