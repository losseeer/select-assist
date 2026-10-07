//! 坐标换算 + 离屏保护。
//! settings.json 里的 windowX/windowY 是「左上角全局坐标」（Electron `getBounds()` 的约定），
//! Cocoa 的窗口 frame 是「左下角、y 向上」；两版共用同一个文件，所以只在边界处换算。

use objc2::MainThreadMarker;
use objc2_app_kit::NSScreen;
use objc2_foundation::{NSPoint, NSRect};

/// 左上角全局坐标下的矩形
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

#[derive(Clone, Copy, Debug)]
pub struct Display {
    /// 整块屏幕，用来挑最近的那块
    pub frame: Rect,
    /// 去掉菜单栏/Dock 的工作区，用来夹位置
    pub work: Rect,
}

/// 与 Electron clampToDisplay 一致：贴边留 8px
pub const EDGE: f64 = 8.0;

/// 显示器断掉后，保存的位置不能把窗口停在屏幕外（照搬 main/index.ts:156）
pub fn clamp_to_display(b: Rect, displays: &[Display]) -> Rect {
    let Some(display) = nearest(displays, b.x, b.y) else {
        return b;
    };
    let work = display.work;
    Rect {
        x: work.x.max(b.x.min(work.x + work.w - b.w - EDGE)),
        y: work.y.max(b.y.min(work.y + work.h - b.h - EDGE)),
        ..b
    }
}

fn nearest(displays: &[Display], x: f64, y: f64) -> Option<&Display> {
    displays
        .iter()
        .min_by(|a, b| distance(&a.frame, x, y).total_cmp(&distance(&b.frame, x, y)))
}

fn distance(frame: &Rect, x: f64, y: f64) -> f64 {
    let dx = (frame.x - x).max(x - (frame.x + frame.w)).max(0.0);
    let dy = (frame.y - y).max(y - (frame.y + frame.h)).max(0.0);
    dx.hypot(dy)
}

#[derive(Clone, Debug)]
pub struct Geometry {
    /// 主屏（带菜单栏那块）的高度：Cocoa 的 y=0 在它的最底边
    pub primary_height: f64,
    pub displays: Vec<Display>,
}

fn from_cocoa(r: NSRect, primary_height: f64) -> Rect {
    Rect {
        x: r.origin.x,
        y: primary_height - (r.origin.y + r.size.height),
        w: r.size.width,
        h: r.size.height,
    }
}

impl Geometry {
    pub fn current(mtm: MainThreadMarker) -> Self {
        let screens = NSScreen::screens(mtm);
        // AppKit 保证 screens[0] 就是主屏
        let primary_height = screens
            .firstObject()
            .map_or(0.0, |screen| screen.frame().size.height);
        let displays = (0..screens.count())
            .map(|index| {
                let screen = screens.objectAtIndex(index);
                Display {
                    frame: from_cocoa(screen.frame(), primary_height),
                    work: from_cocoa(screen.visibleFrame(), primary_height),
                }
            })
            .collect();
        Self {
            primary_height,
            displays,
        }
    }

    /// 窗口左上角 → Cocoa 窗口左下角
    pub fn cocoa_origin(&self, x: f64, top_y: f64, height: f64) -> NSPoint {
        NSPoint::new(x, self.primary_height - top_y - height)
    }

    /// Cocoa 窗口 frame → 存进 settings 的左上角坐标
    pub fn top_left(&self, frame: NSRect) -> Rect {
        from_cocoa(frame, self.primary_height)
    }

    pub fn clamp(&self, r: Rect) -> Rect {
        clamp_to_display(r, &self.displays)
    }

    /// 离某个左上角坐标最近的那块屏（Electron 的 getDisplayNearestPoint）
    pub fn display_near(&self, x: f64, y: f64) -> Option<&Display> {
        nearest(&self.displays, x, y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn display(x: f64, y: f64, w: f64, h: f64) -> Display {
        Display {
            frame: Rect { x, y, w, h },
            work: Rect {
                x,
                y: y + 25.0,
                w,
                h: h - 25.0,
            },
        }
    }

    #[test]
    fn inside_position_is_untouched() {
        let displays = [display(0.0, 0.0, 1470.0, 956.0)];
        let r = Rect {
            x: 221.0,
            y: 132.0,
            w: 400.0,
            h: 44.0,
        };
        assert_eq!(clamp_to_display(r, &displays), r);
    }

    #[test]
    fn offscreen_after_a_display_was_unplugged_gets_pulled_back() {
        let displays = [display(0.0, 0.0, 1470.0, 956.0)];
        let work = displays[0].work;
        let clamped = clamp_to_display(
            Rect {
                x: 99_999.0,
                y: 99_999.0,
                w: 400.0,
                h: 44.0,
            },
            &displays,
        );
        assert_eq!(clamped.x, work.x + work.w - 400.0 - EDGE);
        assert_eq!(clamped.y, work.y + work.h - 44.0 - EDGE);
        // 顶部越过菜单栏时也只回到 work 区内侧，不越过 work.y
        assert_eq!(
            clamp_to_display(
                Rect {
                    x: 10.0,
                    y: -500.0,
                    w: 400.0,
                    h: 44.0
                },
                &displays
            )
            .y,
            work.y
        );
    }

    /// 副屏在左侧时坐标是负的：拿 work.width 直接比会把窗口拽回主屏
    #[test]
    fn negative_x_secondary_display_keeps_its_own_origin() {
        let displays = [
            display(0.0, 0.0, 1470.0, 956.0),
            display(-1920.0, 0.0, 1920.0, 1080.0),
        ];
        let on_secondary = Rect {
            x: -1000.0,
            y: 60.0,
            w: 400.0,
            h: 44.0,
        };
        assert_eq!(clamp_to_display(on_secondary, &displays), on_secondary);

        let past_left_edge = Rect {
            x: -4000.0,
            y: 60.0,
            w: 400.0,
            h: 44.0,
        };
        assert_eq!(clamp_to_display(past_left_edge, &displays).x, -1920.0);
    }

    #[test]
    fn no_displays_leaves_the_position_alone() {
        let r = Rect {
            x: 1.0,
            y: 2.0,
            w: 400.0,
            h: 44.0,
        };
        assert_eq!(clamp_to_display(r, &[]), r);
    }

    /// 面板高度要按「窗口所在那块屏」夹，不是永远按 screens[0]
    #[test]
    fn the_nearest_display_wins_not_the_primary_one() {
        let geometry = Geometry {
            primary_height: 956.0,
            displays: vec![
                display(0.0, 0.0, 1470.0, 956.0),
                display(-1920.0, 0.0, 1920.0, 1080.0),
            ],
        };
        assert_eq!(
            geometry.display_near(-1000.0, 60.0).map(|d| d.frame.w),
            Some(1920.0),
            "副屏在左边（负 x）也要认得"
        );
        assert_eq!(
            geometry.display_near(100.0, 60.0).map(|d| d.frame.w),
            Some(1470.0)
        );
        let empty = Geometry {
            primary_height: 0.0,
            displays: Vec::new(),
        };
        assert!(empty.display_near(0.0, 0.0).is_none());
    }
}
