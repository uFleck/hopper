//! The global summon hotkey. A taken hotkey must never stop Hopper starting: the tray still works.

use hopper_core::{panel::Event, service::HopperError};
use tauri::AppHandle;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

pub const DEFAULT_HOTKEY: &str = "ctrl+alt+h";

pub fn register(app: &AppHandle, hotkey: &str) -> Result<(), String> {
    app.global_shortcut()
        .on_shortcut(hotkey, |app, _shortcut, event| {
            if event.state == ShortcutState::Pressed {
                crate::panel::dispatch(app, Event::Summon);
            }
        })
        .map_err(|e| e.to_string())
}

/// Registers the saved hotkey at startup, falling back to the default; failures are only logged.
pub fn register_at_startup(app: &AppHandle, saved: &str) {
    if let Err(err) = register(app, saved) {
        eprintln!("hopper: could not register {saved}: {err}");
        if !saved.eq_ignore_ascii_case(DEFAULT_HOTKEY)
            && let Err(err) = register(app, DEFAULT_HOTKEY)
        {
            eprintln!("hopper: could not register {DEFAULT_HOTKEY}: {err}");
        }
    }
}

/// Switches from `old` to `new`. The new one is registered first, so on failure the old one stays active.
pub fn replace(app: &AppHandle, old: &str, new: &str) -> Result<(), HopperError> {
    if old.eq_ignore_ascii_case(new) {
        return Ok(());
    }
    register(app, new).map_err(|e| {
        HopperError::invalid(format!("could not register {new}: {e}"))
            .hint("try another combination; this one may be in use by another app")
    })?;
    let _ = app.global_shortcut().unregister(old);
    Ok(())
}
