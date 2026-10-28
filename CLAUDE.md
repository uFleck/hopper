# Hopper

## Rules

- **Windows first.** Test and ship on Windows before anything else.
- **Companion panel, no main window.** The app builds one panel window in Rust (`app.windows: []` in `tauri.conf.json`) and a tray icon. Do not add a main window, a taskbar entry or an Alt+Tab entry. Settings, environments and agent approvals are sheets inside the panel.
- **No native dialogs except the folder picker.** No `ask`, `message`, `confirm` or `alert` from `@tauri-apps/plugin-dialog` or the browser. Use the in-app dialog. The folder picker (`open`) is the only native dialog allowed. `dialog:allow-message` is deliberately not granted.
- **Secrets never reach the frontend.** Secret values stay in Rust (`hopper-core` secrets). The UI only sees status, never values.
- **Policy lives in `hopper-core` services, not in adapters.** `src-tauri` IPC commands and MCP tools stay thin and call the services; they must not decide access.
- **Fast test loop:** `cargo test -p hopper-core`. It does not compile Tauri.
- **Use pnpm.** Node 24.4.1 (`.node-version`).

- **In-app updater:** `src/updater.ts` is the single source of update state (UI reads it with `useSyncExternalStore`); Rust only registers the updater plugin. `release.yml` builds and signs on a `v*` tag; the signing key is the `TAURI_SIGNING_PRIVATE_KEY` secret of the `Production` environment.
