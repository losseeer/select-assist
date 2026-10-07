mod chip;

use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};

fn main() {
    let mtm = MainThreadMarker::new().expect("AppKit needs the main thread");
    let app = NSApplication::sharedApplication(mtm);
    // 与 Electron 版的常驻圆点一样：不占 Dock、不进 ⌘Tab
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

    let _chip = chip::create(mtm);
    app.run();
}
