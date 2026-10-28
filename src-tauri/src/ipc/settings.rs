//! Settings are stored by the service; the side effects (hotkey, autostart) are applied here.

use hopper_core::{panel::Event, service::dto::Settings};
use tauri::{AppHandle, State};
use tauri_plugin_autostart::ManagerExt;

use crate::{
    hotkey, panel,
    state::{AppState, CmdResult},
};

#[tauri::command]
pub async fn settings_get(state: State<'_, AppState>) -> CmdResult<Settings> {
    state.svc.settings_get().await
}

#[tauri::command]
pub async fn settings_set(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: Settings,
) -> CmdResult<Settings> {
    let old = state.svc.settings_get().await?;
    let new_key = settings.hotkey.clone();
    hotkey::replace(&app, &old.hotkey, &new_key)?;
    let saved = match state.svc.settings_set(settings).await {
        Ok(saved) => saved,
        Err(e) => {
            let _ = hotkey::replace(&app, &new_key, &old.hotkey);
            return Err(e);
        }
    };
    panel::dispatch(&app, Event::SetAlwaysShowTab(saved.always_show_tab));
    panel::set_width(&app, saved.width_preset);
    if let Some(on) = saved.start_with_windows {
        apply_autostart(&app, on);
    }
    // The theme is stored only; the page applies it.
    Ok(saved)
}

fn apply_autostart(app: &AppHandle, on: bool) -> bool {
    let launch = app.autolaunch();
    let result = match (on, launch.is_enabled()) {
        (true, Ok(false)) => launch.enable(),
        (false, Ok(true)) => launch.disable(),
        (_, Ok(_)) => Ok(()),
        (_, Err(e)) => Err(e),
    };
    if let Err(e) = &result {
        eprintln!("hopper: could not update start with Windows: {e}");
    }
    result.is_ok()
}

/// First run: start with Windows defaults to on once the first workspace is open. `None` means undecided, so
/// this only acts while the setting is undecided. Debug builds skip it so a dev binary is never registered.
pub async fn first_run_autostart(app: &AppHandle, state: &AppState) {
    if cfg!(debug_assertions) {
        return;
    }
    let Ok(mut settings) = state.svc.settings_get().await else {
        return;
    };
    if settings.start_with_windows.is_some() {
        return;
    }
    if apply_autostart(app, true) {
        settings.start_with_windows = Some(true);
        let _ = state.svc.settings_set(settings).await;
    }
}
