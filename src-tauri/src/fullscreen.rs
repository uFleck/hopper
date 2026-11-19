//! Hides the panel while a fullscreen app (a game, a video, a slideshow) is in front on the panel's monitor, and
//! brings it back after. Event driven: WinEvent hooks on foreground and window location changes, no polling.
//! `hopper_core::fullscreen` decides; this module only reads the windows with thin Win32 calls.

#[cfg(windows)]
mod win {
    use std::sync::OnceLock;

    use hopper_core::fullscreen::{Rect, Window, hides_panel};
    use tauri::{AppHandle, Manager};
    use windows_sys::Win32::Foundation::{HWND, RECT};
    use windows_sys::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
    };
    use windows_sys::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CHILDID_SELF, EVENT_OBJECT_LOCATIONCHANGE, EVENT_SYSTEM_FOREGROUND, GWL_STYLE,
        GetClassNameW, GetForegroundWindow, GetWindowLongPtrW, GetWindowRect,
        GetWindowThreadProcessId, IsIconic, IsZoomed, OBJID_WINDOW, WINEVENT_OUTOFCONTEXT,
        WS_CAPTION,
    };

    use crate::panel;

    /// The callback gets no user data, so it finds the app here.
    static APP: OnceLock<AppHandle> = OnceLock::new();

    /// Starts watching, once the panel window is built. Installs the hooks on the calling thread, which must pump
    /// messages (the main thread does), and checks once for a fullscreen app that is already in front.
    pub fn watch(app: &AppHandle) {
        if APP.set(app.clone()).is_err() {
            return;
        }
        // Kept for the life of the process: they go away with it.
        for event in [EVENT_SYSTEM_FOREGROUND, EVENT_OBJECT_LOCATIONCHANGE] {
            unsafe {
                SetWinEventHook(
                    event,
                    event,
                    std::ptr::null_mut(),
                    Some(on_event),
                    0,
                    0,
                    WINEVENT_OUTOFCONTEXT,
                )
            };
        }
        evaluate(app);
    }

    unsafe extern "system" fn on_event(
        _hook: HWINEVENTHOOK,
        event: u32,
        hwnd: HWND,
        object: i32,
        child: i32,
        _thread: u32,
        _time: u32,
    ) {
        let Some(app) = APP.get() else { return };
        // Location changes also fire for carets and cursors and for every other window: only the foreground
        // window (entering or leaving fullscreen) and the panel (moved to another monitor) matter.
        if event == EVENT_OBJECT_LOCATIONCHANGE
            && (object != OBJID_WINDOW
                || child != CHILDID_SELF as i32
                || (hwnd != unsafe { GetForegroundWindow() } && Some(hwnd) != panel_hwnd(app)))
        {
            return;
        }
        evaluate(app);
    }

    fn panel_hwnd(app: &AppHandle) -> Option<HWND> {
        Some(app.get_webview_window(panel::LABEL)?.hwnd().ok()?.0)
    }

    fn evaluate(app: &AppHandle) {
        let Some(ours) = panel_hwnd(app) else { return };
        let front = unsafe { GetForegroundWindow() };
        let hide = !front.is_null() && fullscreen_over(front, ours);
        panel::set_suppressed(app, hide);
    }

    /// Whether `front` is a fullscreen app on the monitor `panel` is on.
    fn fullscreen_over(front: HWND, panel: HWND) -> bool {
        let mut pid = 0;
        let mut class = [0u16; 256];
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            rcMonitor: rect,
            rcWork: rect,
            dwFlags: 0,
        };
        unsafe {
            GetWindowThreadProcessId(front, &mut pid);
            if GetWindowRect(front, &mut rect) == 0
                || GetMonitorInfoW(
                    MonitorFromWindow(panel, MONITOR_DEFAULTTONEAREST),
                    &mut info,
                ) == 0
            {
                return false;
            }
            let len = GetClassNameW(front, class.as_mut_ptr(), class.len() as i32).max(0);
            let class = String::from_utf16_lossy(&class[..len as usize]);
            let style = GetWindowLongPtrW(front, GWL_STYLE) as u32;
            hides_panel(
                &Window {
                    class: &class,
                    ours: pid == std::process::id(),
                    rect: to_rect(rect),
                    minimized: IsIconic(front) != 0,
                    maximized: IsZoomed(front) != 0,
                    captioned: style & WS_CAPTION == WS_CAPTION,
                },
                &to_rect(info.rcMonitor),
            )
        }
    }

    fn to_rect(r: RECT) -> Rect {
        Rect {
            left: r.left,
            top: r.top,
            right: r.right,
            bottom: r.bottom,
        }
    }
}

#[cfg(windows)]
pub use win::watch;

#[cfg(not(windows))]
pub fn watch(_app: &tauri::AppHandle) {}
