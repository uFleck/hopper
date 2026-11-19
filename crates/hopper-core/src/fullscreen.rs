//! When a fullscreen app (a game, a video, a slideshow) is in front on the panel's monitor, the panel gets out of
//! the way. This is the pure rule; the Tauri shell reads the foreground window with Win32 and asks it.

/// A screen rectangle in physical pixels, right and bottom exclusive (as Win32 `RECT`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    /// Whether this rectangle reaches every edge of `other`.
    pub fn covers(&self, other: &Rect) -> bool {
        self.left <= other.left
            && self.top <= other.top
            && self.right >= other.right
            && self.bottom >= other.bottom
    }
}

/// The foreground window, as much of it as the rule needs.
#[derive(Debug, Clone, Copy)]
pub struct Window<'a> {
    pub class: &'a str,
    /// It belongs to Hopper's own process.
    pub ours: bool,
    pub rect: Rect,
    pub minimized: bool,
    pub maximized: bool,
    /// It has a title bar (`WS_CAPTION`).
    pub captioned: bool,
}

/// The desktop and taskbar windows: the desktop covers the monitor but is not an app in fullscreen.
const SHELL_CLASSES: &[&str] = &[
    "Progman",
    "WorkerW",
    "Shell_TrayWnd",
    "Shell_SecondaryTrayWnd",
];

/// Whether `front` is a fullscreen app on the monitor whose full bounds are `monitor`.
///
/// A maximized window with a title bar is not fullscreen even when it covers the monitor (an auto-hidden taskbar
/// leaves it the whole screen, and its frame overhangs every edge).
pub fn hides_panel(front: &Window, monitor: &Rect) -> bool {
    if front.ours || front.minimized || SHELL_CLASSES.contains(&front.class) {
        return false;
    }
    if front.maximized && front.captioned {
        return false;
    }
    front.rect.covers(monitor)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MONITOR: Rect = Rect {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1080,
    };
    const SECOND: Rect = Rect {
        left: 1920,
        top: 0,
        right: 4480,
        bottom: 1440,
    };

    fn window(rect: Rect) -> Window<'static> {
        Window {
            class: "UnityWndClass",
            ours: false,
            rect,
            minimized: false,
            maximized: false,
            captioned: false,
        }
    }

    #[test]
    fn a_borderless_window_covering_the_monitor_hides_the_panel() {
        assert!(hides_panel(&window(MONITOR), &MONITOR));
    }

    #[test]
    fn a_maximized_window_with_a_title_bar_does_not() {
        // Taskbar visible: the work area stops short of the bottom edge.
        let mut front = window(Rect {
            left: -8,
            top: -8,
            right: 1928,
            bottom: 1048,
        });
        front.maximized = true;
        front.captioned = true;
        assert!(!hides_panel(&front, &MONITOR));
        // Taskbar auto-hidden: the frame overhangs every edge, still not fullscreen.
        front.rect = Rect {
            left: -8,
            top: -8,
            right: 1928,
            bottom: 1088,
        };
        assert!(!hides_panel(&front, &MONITOR));
    }

    #[test]
    fn a_maximized_window_without_a_title_bar_is_fullscreen() {
        // Browsers and players drop the caption and maximize for F11 or a fullscreen video.
        let mut front = window(MONITOR);
        front.maximized = true;
        assert!(hides_panel(&front, &MONITOR));
    }

    #[test]
    fn a_window_smaller_than_the_monitor_does_not() {
        let front = window(Rect {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1079,
        });
        assert!(!hides_panel(&front, &MONITOR));
    }

    #[test]
    fn only_a_fullscreen_window_on_the_panels_monitor_counts() {
        let on_second = window(SECOND);
        assert!(!hides_panel(&on_second, &MONITOR));
        assert!(hides_panel(&on_second, &SECOND));
        assert!(!hides_panel(&window(MONITOR), &SECOND));
    }

    #[test]
    fn the_desktop_taskbar_and_hopper_itself_never_count() {
        for class in SHELL_CLASSES {
            let mut front = window(MONITOR);
            front.class = class;
            assert!(!hides_panel(&front, &MONITOR), "{class}");
        }
        let mut ours = window(MONITOR);
        ours.ours = true;
        assert!(!hides_panel(&ours, &MONITOR));
    }

    #[test]
    fn a_minimized_window_does_not() {
        let mut front = window(MONITOR);
        front.minimized = true;
        assert!(!hides_panel(&front, &MONITOR));
    }
}
