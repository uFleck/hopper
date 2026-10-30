use hopper_core::{panel::Event, userdata::DockSide};
use serde_json::json;
use tauri::menu::{CheckMenuItem, Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, RunEvent, WindowEvent};
use tauri_plugin_autostart::MacosLauncher;

mod hotkey;
mod ipc;
mod mcp;
mod panel;
mod state;

/// The tray is the only way to quit Hopper, and the fallback when the hotkey is taken.
fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let toggle = MenuItem::with_id(app, "toggle", "Show / Hide", true, None::<&str>)?;
    let pin = CheckMenuItem::with_id(app, "pin", "Pin", true, false, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)?;
    let updates = MenuItem::with_id(app, "updates", "Check for updates", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&toggle, &pin, &settings, &updates, &quit])?;
    let tray = TrayIconBuilder::with_id("main")
        .icon(tauri::include_image!("icons/tray.png"))
        .tooltip("Hopper")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "toggle" => panel::dispatch(app, Event::Summon),
            "pin" => panel::dispatch(app, Event::TogglePin),
            "settings" => {
                panel::dispatch(app, Event::Summon);
                let _ = app.emit("panel://open-sheet", json!({ "sheet": "settings" }));
            }
            "updates" => {
                // The panel runs the check and shows the answer in Settings, so no native dialog is needed.
                panel::dispatch(app, Event::Summon);
                let _ = app.emit("panel://open-sheet", json!({ "sheet": "settings" }));
                let _ = app.emit("update://check", ());
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                panel::dispatch(tray.app_handle(), Event::Summon);
            }
        });
    tray.build(app)?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            panel::dispatch(app, Event::Summon);
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            panel::panel_place,
            panel::panel_drag,
            panel::panel_pointer,
            panel::panel_event,
            panel::panel_mode,
            ipc::workspace::workspace_current,
            ipc::workspace::workspace_recent,
            ipc::workspace::workspace_known,
            ipc::workspace::workspace_discover,
            ipc::workspace::workspace_open,
            ipc::workspace::workspace_init,
            ipc::workspace::workspace_close,
            ipc::workspace::workspace_pick_folder,
            ipc::workspace::tree_get,
            ipc::workspace::collection_create,
            ipc::workspace::folder_create,
            ipc::workspace::recent_list,
            ipc::request::request_get,
            ipc::request::request_save,
            ipc::request::request_delete,
            ipc::request::request_undelete,
            ipc::request::request_move,
            ipc::request::request_duplicate,
            ipc::request::operation_open,
            ipc::env::environment_list,
            ipc::env::environment_save,
            ipc::env::environment_delete,
            ipc::env::environment_set_active,
            ipc::env::environment_legacy_list,
            ipc::env::environment_migrate_legacy,
            ipc::env::variables_get,
            ipc::env::variables_save,
            ipc::env::collection_auth_get,
            ipc::env::collection_auth_save,
            ipc::secrets::secret_set,
            ipc::secrets::secret_clear,
            ipc::secrets::secret_reveal,
            ipc::spec::spec_import,
            ipc::spec::search_routes,
            ipc::run::request_send,
            ipc::run::request_cancel,
            ipc::run::copy_curl,
            ipc::history::history_list,
            ipc::history::history_get,
            ipc::history::history_all,
            ipc::history::history_label,
            ipc::history::history_delete,
            ipc::history::history_request,
            ipc::history::history_clear,
            ipc::history::cookies_list,
            ipc::history::cookies_clear,
            ipc::agent::agent_access_set,
            ipc::agent::agent_grants_list,
            ipc::agent::agent_env_rules_set,
            ipc::agent::agent_activity_list,
            ipc::agent::approval_pending,
            ipc::agent::approval_respond,
            ipc::agent::mcp_status,
            ipc::agent::mcp_setup,
            ipc::agent::mcp_connect_claude,
            ipc::settings::settings_get,
            ipc::settings::settings_set,
        ])
        .on_window_event(|window, event| {
            if window.label() == panel::LABEL
                && let WindowEvent::Focused(false) = event
            {
                panel::dispatch(window.app_handle(), Event::FocusLost);
            }
        })
        .setup(|app| {
            #[cfg(desktop)]
            app.handle()
                .plugin(tauri_plugin_updater::Builder::new().build())?;
            let app_state = state::build(app.handle())?;
            let settings = tauri::async_runtime::block_on(app_state.svc.settings_get())
                .map_err(|e| e.message)?;
            app.manage(
                panel::Panel::new(settings.always_show_tab, false)
                    .with_width(settings.width_preset)
                    .with_dock_file(
                        app.path().app_data_dir()?.join("dock.json"),
                        settings.dock_side == DockSide::Right,
                    ),
            );
            mcp::start(
                app.handle().clone(),
                app_state.mcp.clone(),
                &app_state.svc,
                app_state.userdata.clone(),
            );
            // Bring back the workspace that was open last time.
            tauri::async_runtime::block_on(app_state.svc.workspace_restore());
            app.manage(app_state);
            build_tray(app.handle())?;
            panel::init(app.handle());
            hotkey::register_at_startup(app.handle(), &settings.hotkey);
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building Hopper");

    app.run(|_app, event| {
        // With no window shown, closing the panel must not end the process. Only the tray's Quit (`app.exit`) does.
        if let RunEvent::ExitRequested {
            code: None, api, ..
        } = event
        {
            api.prevent_exit();
        }
    });
}
