use hopper_core::service::{
    HopperError,
    dto::{DiscoveredWorkspace, RecentItem, RecentWorkspace, TreeDto, WorkspaceInfo},
};
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;

use super::settings::first_run_autostart;
use crate::{
    panel,
    state::{AppState, CmdResult},
};

#[tauri::command]
pub async fn workspace_current(state: State<'_, AppState>) -> CmdResult<Option<WorkspaceInfo>> {
    state.svc.workspace_current().await
}

#[tauri::command]
pub async fn workspace_recent(state: State<'_, AppState>) -> CmdResult<Vec<RecentWorkspace>> {
    state.svc.workspace_recent().await
}

#[tauri::command]
pub async fn workspace_discover(state: State<'_, AppState>) -> CmdResult<Vec<DiscoveredWorkspace>> {
    state.svc.workspace_discover().await
}

#[tauri::command]
pub async fn workspace_known(state: State<'_, AppState>) -> CmdResult<Vec<DiscoveredWorkspace>> {
    state.svc.workspace_known().await
}

#[tauri::command]
pub async fn workspace_open(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> CmdResult<WorkspaceInfo> {
    let info = state.svc.workspace_open(&path).await?;
    first_run_autostart(&app, &state).await;
    Ok(info)
}

#[tauri::command]
pub async fn workspace_init(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    name: String,
) -> CmdResult<WorkspaceInfo> {
    let info = state.svc.workspace_init(&path, &name).await?;
    first_run_autostart(&app, &state).await;
    Ok(info)
}

#[tauri::command]
pub async fn workspace_close(state: State<'_, AppState>) -> CmdResult<()> {
    state.svc.workspace_close().await
}

/// The only native dialog. The panel hides on focus loss, so that rule is suspended while the picker is open and
/// the panel gets focus back afterwards.
#[tauri::command]
pub async fn workspace_pick_folder(app: AppHandle) -> CmdResult<Option<String>> {
    panel::picker(&app, true);
    let handle = app.clone();
    let picked = tauri::async_runtime::spawn_blocking(move || {
        let dialog = handle.dialog().file();
        let dialog = match handle.get_webview_window(panel::LABEL) {
            Some(window) => dialog.set_parent(&window),
            None => dialog,
        };
        dialog.blocking_pick_folder()
    })
    .await;
    panel::picker(&app, false);
    let picked = picked.map_err(|e| HopperError::internal(e.to_string()))?;
    Ok(picked
        .and_then(|p| p.into_path().ok())
        .map(|p| p.display().to_string()))
}

#[tauri::command]
pub async fn tree_get(state: State<'_, AppState>) -> CmdResult<TreeDto> {
    state.svc.tree_get().await
}

#[tauri::command]
pub async fn collection_create(state: State<'_, AppState>, name: String) -> CmdResult<String> {
    state.svc.collection_create(&name).await
}

#[tauri::command]
pub async fn folder_create(
    state: State<'_, AppState>,
    collection: String,
    parent: Vec<String>,
    name: String,
) -> CmdResult<String> {
    state.svc.folder_create(&collection, &parent, &name).await
}

#[tauri::command]
pub async fn recent_list(state: State<'_, AppState>) -> CmdResult<Vec<RecentItem>> {
    state.svc.recent_list().await
}
