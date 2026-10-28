//! The companion panel window: built in Rust at startup (there is no main window), shown without activation.
//! It is stuck to the left or right edge of the screen (Circuit): a handle on the edge, a pill at the top corner, or
//! the full panel 16 px in from the edge. Dragging it lets go of the edge only until the button is released, then
//! it snaps to the nearest one. *When* it is shown or focused is decided by `hopper_core::panel::PanelState`; this
//! module only executes the outcome with thin Win32 calls (ported from Spawner's `companion.rs`).

use std::{
    path::PathBuf,
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
    time::Duration,
};

use hopper_core::panel::{Event, Focus, Mode, Outcome, PanelState};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

pub const LABEL: &str = "panel";

/// Logical sizes per mode. The open panel's width is the `widthPreset` setting (430, 640 or 860); this is the default.
const WIDTH: u32 = 640;
/// The handle on the screen edge: it is clipped by the window, so the page slides it half out of sight.
const TAB: (f64, f64) = (52.0, 64.0);
/// Gap between the pill or the open panel and the screen edge.
const MARGIN: f64 = 16.0;
/// The notice pill at the top corner. Narrow on purpose: the transparent rest of the window still takes clicks.
const COMPACT: (f64, f64) = (360.0, 52.0);
/// How far a press has to move to count as a drag and not a click, in physical pixels.
const DRAG_SLOP: i32 = 4;
/// How long the open panel animates out before the window shrinks to the handle. Matches `panel-out` in panel.css.
const COLLAPSE: Duration = Duration::from_millis(280);

/// Which edge the panel is stuck to, and where the handle sits along it. Kept between runs.
#[derive(Clone, Copy, Serialize, Deserialize)]
struct Dock {
    right: bool,
    /// Vertical centre of the handle, as a fraction of the work area's height.
    tab: f64,
}

pub struct Panel {
    state: Mutex<PanelState>,
    dock: Mutex<Dock>,
    dock_file: Option<PathBuf>,
    /// Logical width of the open panel, from the `widthPreset` setting.
    width: AtomicU32,
    /// The native folder picker is open: losing focus to it must not hide the panel.
    picker_open: AtomicBool,
    /// The OS move loop is running: the window churns focus while it is dragged, and that is not the user leaving.
    dragging: AtomicBool,
    /// The mode last put on screen. `state` moves ahead of it while the panel animates out.
    shown: Mutex<Mode>,
}

impl Panel {
    pub fn new(always_show_tab: bool, pinned: bool) -> Self {
        Self {
            state: Mutex::new(PanelState::new(always_show_tab, pinned)),
            dock: Mutex::new(Dock {
                right: true,
                tab: 0.5,
            }),
            dock_file: None,
            width: AtomicU32::new(WIDTH),
            picker_open: AtomicBool::new(false),
            dragging: AtomicBool::new(false),
            shown: Mutex::new(Mode::Hidden),
        }
    }

    /// Restores the edge saved in `file` (else `default_right`) and keeps saving there.
    pub fn with_dock_file(mut self, file: PathBuf, default_right: bool) -> Self {
        let saved = std::fs::read_to_string(&file)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok());
        *self.dock.lock().unwrap() = saved.unwrap_or(Dock {
            right: default_right,
            tab: 0.5,
        });
        self.dock_file = Some(file);
        self
    }

    /// Sets the open panel's width (logical px) from the `widthPreset` setting.
    pub fn with_width(self, width: u32) -> Self {
        self.width.store(width, Ordering::Relaxed);
        self
    }

    fn set_dock(&self, dock: Dock) {
        *self.dock.lock().unwrap() = dock;
        let Some(file) = &self.dock_file else {
            return;
        };
        if let Ok(text) = serde_json::to_string(&dock) {
            let _ = std::fs::write(file, text);
        }
    }
}

/// The current visibility mode (hidden, tab, compact, open).
pub fn mode(app: &AppHandle) -> Mode {
    app.try_state::<Panel>()
        .map_or(Mode::Hidden, |p| p.state.lock().unwrap().mode())
}

/// Brackets the folder picker: while it is open the panel stays put, and afterwards it gets focus back.
pub fn picker(app: &AppHandle, open: bool) {
    let Some(panel) = app.try_state::<Panel>() else {
        return;
    };
    panel.picker_open.store(open, Ordering::Relaxed);
    if !open && mode(app) == Mode::Open {
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || {
            if let Some(window) = window(&handle) {
                set_focus(&window, true);
            }
        });
    }
}

/// Applies a saved `widthPreset` and, if the panel is open, resizes it now.
pub fn set_width(app: &AppHandle, width: u32) {
    let Some(panel) = app.try_state::<Panel>() else {
        return;
    };
    panel.width.store(width, Ordering::Relaxed);
    if mode(app) == Mode::Open {
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || {
            if let Some(window) = window(&handle) {
                dock(&window, Mode::Open);
            }
        });
    }
}

/// Feeds an event to the state machine and performs what it asks for, on the main thread.
pub fn dispatch(app: &AppHandle, event: Event) {
    let Some(panel) = app.try_state::<Panel>() else {
        return;
    };
    if event == Event::FocusLost
        && (panel.picker_open.load(Ordering::Relaxed) || panel.dragging.load(Ordering::Relaxed))
    {
        return;
    }
    // Moving the window makes the webview report lost focus while the window is still the foreground one.
    if event == Event::FocusLost
        && app
            .get_webview_window(LABEL)
            .is_some_and(|w| is_foreground(&w))
    {
        return;
    }
    let outcome = panel.state.lock().unwrap().step(event);
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || apply(&handle, outcome));
}

fn apply(app: &AppHandle, outcome: Outcome) {
    let Some(win) = window(app) else { return };
    if outcome.mode_changed {
        let was = app.try_state::<Panel>().map_or(Mode::Hidden, |p| {
            std::mem::replace(&mut *p.shown.lock().unwrap(), outcome.mode)
        });
        if was == Mode::Open && outcome.mode != Mode::Open {
            // The page plays its exit while the window is still full size; only then does the window shrink.
            let _ = app.emit("panel://collapsing", ());
            let handle = app.clone();
            std::thread::spawn(move || {
                std::thread::sleep(COLLAPSE);
                let again = handle.clone();
                let _ = handle.run_on_main_thread(move || {
                    if let Some(win) = window(&again) {
                        settle(&again, &win, mode(&again));
                    }
                });
            });
        } else {
            settle(app, &win, outcome.mode);
        }
    }
    match outcome.focus {
        Focus::Take => set_focus(&win, true),
        Focus::Release => set_focus(&win, false),
        Focus::Unchanged => {}
    }
    if outcome.focus == Focus::Take {
        let _ = app.emit("panel://summon", ());
    }
}

/// Puts the window in its place for `mode` and tells the page.
fn settle(app: &AppHandle, window: &WebviewWindow, mode: Mode) {
    match mode {
        Mode::Hidden => set_visible(window, false),
        mode => {
            dock(window, mode);
            set_visible(window, true);
        }
    }
    emit_mode(app, window, mode);
}

/// The panel window, built hidden the first time it is needed.
fn window(app: &AppHandle) -> Option<WebviewWindow> {
    if let Some(w) = app.get_webview_window(LABEL) {
        return Some(w);
    }
    let window = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("index.html".into()))
        .title("Hopper")
        .inner_size(TAB.0, TAB.1)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focused(false)
        .focusable(false)
        .visible(false)
        .build()
        .ok()?;
    hide_from_alt_tab(&window);
    Some(window)
}

/// Builds the window at startup so the first summon is instant.
pub fn init(app: &AppHandle) {
    window(app);
}

type Area = tauri::PhysicalRect<i32, u32>;

fn current_dock(window: &WebviewWindow) -> Dock {
    window.app_handle().try_state::<Panel>().map_or(
        Dock {
            right: true,
            tab: 0.5,
        },
        |p| *p.dock.lock().unwrap(),
    )
}

/// The open panel's logical width, from the `widthPreset` setting (`WIDTH` until it is read).
fn current_width(window: &WebviewWindow) -> u32 {
    window
        .app_handle()
        .try_state::<Panel>()
        .map_or(WIDTH, |p| p.width.load(Ordering::Relaxed))
}

/// The work area of the monitor under `point` (physical pixels), else the window's own or the primary one.
fn work_area(window: &WebviewWindow, point: Option<(i32, i32)>) -> Option<(Area, f64)> {
    let under = point.and_then(|(x, y)| {
        window.available_monitors().ok()?.into_iter().find(|m| {
            let r = m.work_area();
            x >= r.position.x
                && y >= r.position.y
                && x < r.position.x + r.size.width as i32
                && y < r.position.y + r.size.height as i32
        })
    });
    let monitor = under.or_else(|| {
        window
            .current_monitor()
            .ok()
            .flatten()
            .or_else(|| window.primary_monitor().ok().flatten())
    })?;
    Some((*monitor.work_area(), monitor.scale_factor()))
}

/// Places the window for `mode` against the docked edge: the handle flush with it, the pill and the panel
/// `MARGIN` in from it (and the panel `MARGIN` from the top and bottom, as in Circuit).
fn dock(window: &WebviewWindow, mode: Mode) {
    let Some((area, scale)) = work_area(window, None) else {
        return;
    };
    let edge = current_dock(window);
    let phys = |logical: f64| (logical * scale).round() as i32;
    let margin = phys(MARGIN);
    let (left, top) = (area.position.x, area.position.y);
    let (right, bottom) = (left + area.size.width as i32, top + area.size.height as i32);
    let handle = matches!(mode, Mode::Tab | Mode::Hidden);
    let (w, h) = match mode {
        Mode::Tab | Mode::Hidden => (phys(TAB.0), phys(TAB.1)),
        Mode::Compact => (phys(COMPACT.0), phys(COMPACT.1)),
        Mode::Open => (
            phys(f64::from(current_width(window))).min(area.size.width as i32 - margin * 2),
            area.size.height as i32 - margin * 2,
        ),
    };
    let inset = if handle { 0 } else { margin };
    let x = if edge.right {
        right - inset - w
    } else {
        left + inset
    };
    let y = if handle {
        top + (edge.tab * f64::from(area.size.height)).round() as i32 - h / 2
    } else {
        top + margin
    };
    let x = x.clamp(left, (right - w).max(left));
    let y = y.clamp(top, (bottom - h).max(top));
    place(window, x, y, w.max(1) as u32, h.max(1) as u32);
}

/// The mode and edge as the page needs them: `panel://mode` and the `panel_mode` command.
#[derive(Clone, Serialize)]
pub struct ModeInfo {
    mode: String,
    right: bool,
}

fn mode_info(window: &WebviewWindow, mode: Mode) -> ModeInfo {
    ModeInfo {
        mode: format!("{mode:?}").to_lowercase(),
        right: current_dock(window).right,
    }
}

fn emit_mode(app: &AppHandle, window: &WebviewWindow, mode: Mode) {
    let _ = app.emit("panel://mode", mode_info(window, mode));
}

/// What the page renders at startup: it may have missed the first `panel://mode`.
#[tauri::command]
pub fn panel_mode(app: AppHandle) -> Option<ModeInfo> {
    let window = app.get_webview_window(LABEL)?;
    Some(mode_info(&window, mode(&app)))
}

/// Shown without activating, so the window the user is typing in keeps focus. Tauri's `show` activates the
/// window, and its hidden flag would go stale after a raw show, so both directions go through Win32 here.
#[cfg(windows)]
fn set_visible(window: &WebviewWindow, show: bool) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{SW_HIDE, SW_SHOWNOACTIVATE, ShowWindow};
    if let Ok(hwnd) = window.hwnd() {
        unsafe { ShowWindow(hwnd.0, if show { SW_SHOWNOACTIVATE } else { SW_HIDE }) };
    }
}

#[cfg(not(windows))]
fn set_visible(window: &WebviewWindow, show: bool) {
    let _ = if show { window.show() } else { window.hide() };
}

/// Moves and sizes the panel in one step, in physical pixels.
#[tauri::command]
pub fn panel_place(app: AppHandle, x: i32, y: i32, width: u32, height: u32) {
    let Some(window) = app.get_webview_window(LABEL) else {
        return;
    };
    place(&window, x, y, width, height);
}

#[cfg(windows)]
fn place(window: &WebviewWindow, x: i32, y: i32, width: u32, height: u32) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SWP_NOACTIVATE, SWP_NOOWNERZORDER, SWP_NOZORDER, SetWindowPos,
    };
    if let Ok(hwnd) = window.hwnd() {
        unsafe {
            SetWindowPos(
                hwnd.0,
                std::ptr::null_mut(),
                x,
                y,
                width as i32,
                height as i32,
                SWP_NOACTIVATE | SWP_NOZORDER | SWP_NOOWNERZORDER,
            )
        };
    }
}

#[cfg(not(windows))]
fn place(window: &WebviewWindow, x: i32, y: i32, width: u32, height: u32) {
    let _ = window.set_position(tauri::PhysicalPosition::new(x, y));
    let _ = window.set_size(tauri::PhysicalSize::new(width, height));
}

/// Lets the user drag the panel off its edge and returns once they let go, with whether it actually moved (a press
/// that did not move is a click). On release it snaps to the nearest screen edge and the new edge is remembered.
#[tauri::command]
pub async fn panel_drag(app: AppHandle) -> bool {
    let (done, dropped) = tokio::sync::oneshot::channel();
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        let moved = handle
            .get_webview_window(LABEL)
            .is_some_and(|window| drag_and_snap(&handle, &window));
        let _ = done.send(moved);
    });
    dropped.await.unwrap_or(false)
}

fn drag_and_snap(app: &AppHandle, window: &WebviewWindow) -> bool {
    let Ok(from) = window.outer_position() else {
        return false;
    };
    let panel = app.try_state::<Panel>();
    if let Some(panel) = &panel {
        panel.dragging.store(true, Ordering::Relaxed);
    }
    drag(window);
    // The focus change the move loop caused is delivered after this returns, so the guard outlives it.
    if panel.is_some() {
        let handle = app.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(400));
            if let Some(panel) = handle.try_state::<Panel>() {
                panel.dragging.store(false, Ordering::Relaxed);
            }
        });
    }
    let mode = mode(app);
    let moved = match (window.outer_position(), window.outer_size()) {
        (Ok(to), Ok(size)) if (to.x - from.x).abs().max((to.y - from.y).abs()) > DRAG_SLOP => {
            if let Some(panel) = &panel {
                snap(panel, window, mode, to, size);
            }
            true
        }
        _ => false,
    };
    dock(window, mode);
    emit_mode(app, window, mode);
    if mode == Mode::Open {
        // The loop may have taken focus away from the panel the user is still working in.
        set_focus(window, true);
    }
    moved
}

/// Sticks the window, dropped at `to`, to the edge of the monitor it is mostly on that is nearest to it.
fn snap(
    panel: &Panel,
    window: &WebviewWindow,
    mode: Mode,
    to: tauri::PhysicalPosition<i32>,
    size: tauri::PhysicalSize<u32>,
) {
    let centre = (to.x + size.width as i32 / 2, to.y + size.height as i32 / 2);
    let Some((area, _)) = work_area(window, Some(centre)) else {
        return;
    };
    let mut edge = current_dock(window);
    edge.right = centre.0 > area.position.x + area.size.width as i32 / 2;
    if matches!(mode, Mode::Tab | Mode::Hidden) {
        let along = f64::from(centre.1 - area.position.y) / f64::from(area.size.height.max(1));
        edge.tab = along.clamp(0.06, 0.94);
    }
    panel.set_dock(edge);
}

/// Tauri's `start_dragging` posts the message and returns at once. Sent instead, the OS move loop runs inside the
/// call, which returns when the mouse button is released.
#[cfg(windows)]
fn drag(window: &WebviewWindow) {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetCursorPos, HTCAPTION, SendMessageW, WM_NCLBUTTONDOWN,
    };
    if let Ok(hwnd) = window.hwnd() {
        let mut at = POINT { x: 0, y: 0 };
        unsafe {
            GetCursorPos(&mut at);
            ReleaseCapture();
            let point = ((at.y as u16 as u32) << 16 | at.x as u16 as u32) as isize;
            SendMessageW(hwnd.0, WM_NCLBUTTONDOWN, HTCAPTION as usize, point);
        }
    }
}

#[cfg(not(windows))]
fn drag(window: &WebviewWindow) {
    let _ = window.start_dragging();
}

/// Where the cursor is inside the panel, in physical pixels from its top left corner, or none when it is outside
/// the window or another window covers it. The page cannot tell this reliably itself: clicking in the panel moves
/// keyboard focus to it, and the webview then reports that the mouse left.
#[tauri::command]
pub fn panel_pointer(app: AppHandle) -> Option<(i32, i32)> {
    let window = app.get_webview_window(LABEL)?;
    pointer(&window)
}

#[cfg(windows)]
fn pointer(window: &WebviewWindow) -> Option<(i32, i32)> {
    use windows_sys::Win32::Foundation::{POINT, RECT};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GA_ROOT, GetAncestor, GetCursorPos, GetWindowRect, WindowFromPoint,
    };
    let hwnd = window.hwnd().ok()?.0;
    unsafe {
        let mut at = POINT { x: 0, y: 0 };
        if GetCursorPos(&mut at) == 0 {
            return None;
        }
        let hit = WindowFromPoint(at);
        if hit.is_null() || GetAncestor(hit, GA_ROOT) != hwnd {
            return None;
        }
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        if GetWindowRect(hwnd, &mut rect) == 0 {
            return None;
        }
        Some((at.x - rect.left, at.y - rect.top))
    }
}

#[cfg(not(windows))]
fn pointer(window: &WebviewWindow) -> Option<(i32, i32)> {
    let cursor = window.cursor_position().ok()?;
    let at = window.outer_position().ok()?;
    let size = window.outer_size().ok()?;
    let (x, y) = (
        (cursor.x - at.x as f64) as i32,
        (cursor.y - at.y as f64) as i32,
    );
    (x >= 0 && y >= 0 && x < size.width as i32 && y < size.height as i32).then_some((x, y))
}

/// Whether the panel window (or its webview) is the foreground window right now.
#[cfg(windows)]
fn is_foreground(window: &WebviewWindow) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GA_ROOT, GetAncestor, GetForegroundWindow};
    let Ok(hwnd) = window.hwnd() else {
        return false;
    };
    let front = unsafe { GetForegroundWindow() };
    !front.is_null() && (front == hwnd.0 || unsafe { GetAncestor(front, GA_ROOT) } == hwnd.0)
}

#[cfg(not(windows))]
fn is_foreground(window: &WebviewWindow) -> bool {
    window.is_focused().unwrap_or(false)
}

/// The window that had focus before the panel took it, so it can get it back.
#[cfg(windows)]
static PREVIOUS: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);

#[cfg(windows)]
fn set_focus(window: &WebviewWindow, focus: bool) {
    use std::sync::atomic::Ordering;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GWL_EXSTYLE, GetForegroundWindow, GetWindowLongPtrW, IsWindow, SetForegroundWindow,
        SetWindowLongPtrW, WS_EX_NOACTIVATE,
    };
    let Ok(hwnd) = window.hwnd() else { return };
    let ours = hwnd.0 as isize;
    // The style is flipped here and not through `set_focusable`: that makes the window layer re-apply the
    // visibility it remembers, and this window is shown and hidden with raw Win32 calls it knows nothing about.
    let style = unsafe { GetWindowLongPtrW(hwnd.0, GWL_EXSTYLE) };
    if focus {
        let front = unsafe { GetForegroundWindow() } as isize;
        if front != ours && front != 0 {
            PREVIOUS.store(front, Ordering::Relaxed);
        }
        unsafe {
            SetWindowLongPtrW(hwnd.0, GWL_EXSTYLE, style & !(WS_EX_NOACTIVATE as isize));
            SetForegroundWindow(hwnd.0);
        }
    } else {
        unsafe { SetWindowLongPtrW(hwnd.0, GWL_EXSTYLE, style | WS_EX_NOACTIVATE as isize) };
        let previous = PREVIOUS.swap(0, Ordering::Relaxed);
        // Only hand focus back when it is still ours; the user may have clicked somewhere else already.
        if previous != 0
            && unsafe { GetForegroundWindow() } as isize == ours
            && unsafe { IsWindow(previous as _) } != 0
        {
            unsafe { SetForegroundWindow(previous as _) };
        }
    }
}

#[cfg(not(windows))]
fn set_focus(window: &WebviewWindow, focus: bool) {
    let _ = window.set_focusable(focus);
    if focus {
        let _ = window.set_focus();
    }
}

/// A tool window is left out of Alt+Tab, which `skip_taskbar` alone does not do.
#[cfg(windows)]
fn hide_from_alt_tab(window: &WebviewWindow) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GWL_EXSTYLE, GetWindowLongPtrW, SetWindowLongPtrW, WS_EX_APPWINDOW, WS_EX_TOOLWINDOW,
    };
    if let Ok(hwnd) = window.hwnd() {
        unsafe {
            let style = GetWindowLongPtrW(hwnd.0, GWL_EXSTYLE);
            SetWindowLongPtrW(
                hwnd.0,
                GWL_EXSTYLE,
                (style | WS_EX_TOOLWINDOW as isize) & !(WS_EX_APPWINDOW as isize),
            );
        }
    }
}

#[cfg(not(windows))]
fn hide_from_alt_tab(_window: &WebviewWindow) {}

/// Events the page may raise: only user gestures inside the panel, never a way to take focus on its own.
#[tauri::command]
pub fn panel_event(app: AppHandle, name: String) {
    let event = match name.as_str() {
        "dismiss" => Event::Dismiss,
        "clicked" => Event::Clicked,
        "hover-enter" => Event::HoverEnter,
        "hover-leave" => Event::HoverLeave,
        "toggle-pin" => Event::TogglePin,
        // The floating Hopper or its notice was clicked: the same as the hotkey.
        "open" => Event::Summon,
        _ => return,
    };
    dispatch(&app, event);
}
