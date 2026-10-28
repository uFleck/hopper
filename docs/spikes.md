# Spikes: results to date

Six spikes from PLAN section 1 and Verification. Status: done, partly done (needs a manual run), or not verified.

## (a) reqwest 0.13 with the rustls platform verifier: test written, manual run needed

reqwest 0.13.5 (`rustls` feature, `rustls-platform-verifier` 0.7) should trust the Windows certificate store. An
automated test needs an internal CA installed on the machine, so it is `#[ignore]`d. To run it on a machine that
trusts an internal CA (see the doc comment in `crates/hopper-core/src/engine/tests.rs`):

    cargo test -p hopper-core internal_ca -- --ignored --nocapture

Fallback if it fails: the `native-tls` backend (SChannel). Not needed so far.

## (b) MCP: hand-rolled, not `rmcp`: done

Decision: copy Spawner's `mcp.rs` pattern (axum 0.8 with `tokio` + `http1` only, Streamable HTTP with JSON responses,
`initialize` / `ping` / `tools/list` / `tools/call`, notifications answered with 202). Why: it is proven in Spawner,
adds no dependency beyond axum, and keeps the Hopper-specific parts (Host/Origin/bearer gate, per-session client name,
`AgentApi` as the only handle) in about 400 lines we can read. `rmcp` would add a typed layer we do not need, since tool
arguments already deserialize into the core arg structs. Tools and input schemas are hand-written in
`src-tauri/src/mcp/tools.rs`. Tests: `src-tauri/src/mcp/tests.rs` (19 tools, gate, conflicts, secrets, approvals).
Known limit: a client disconnect drops the pending request future, but a send already handed to the engine finishes
or times out on its own (the user can cancel it in the panel).

## (c) keyring on Windows 11: done

`keyring` 3.6.3 works against Windows Credential Manager. The real limit is a 2560-byte blob, stored as UTF-16, so
1280 UTF-16 units per secret. The guard lives in `crates/hopper-core/src/secrets/keyring.rs` and returns a clear error.

## (d) YAML: done

`serde_yaml_ng` 0.10 (the maintained fork of the archived `serde_yaml`) with a canonical writer in `format`
(stable key order, idempotent, unknown keys preserved) so AI-written files give clean git diffs.

## (e) TypeScript types: decided, hand-written

Not `tauri-specta` or `ts-rs`. `src/ipc/types.ts` is hand-maintained; Rust DTO fixtures are written to
`fixtures/ipc` by a core test and `src/ipc/contract.test.ts` checks them against the types, so drift fails CI.
Reason: fewer moving parts on Tauri 2 and the types read as the contract, not as generated noise.

## (f) panel behaviours on Windows: not verified by automated tests, manual run needed

The state machine (`hopper_core::panel`) is unit-tested; the Win32 calls in `src-tauri/src/panel.rs` are not. Run
`pnpm tauri dev` and check (PLAN Verification):

- No main window, no taskbar button, no Alt+Tab entry; the tray icon appears.
- `Ctrl+Alt+H` shows the docked panel with the search box focused; Esc (or the hotkey again) hides it and focus
  returns to the previous window.
- A passive show (agent activity, a send, an approval) does not steal focus or keystrokes.
- An approval shows passively, raises an OS notification when the panel was not open, and ignores input for ~500 ms.
- The folder picker keeps the panel on screen while open and the panel is focused again after it closes.
- Changing the hotkey, dock side or width in Settings takes effect at once; a taken hotkey keeps the old one.
- Tray: Settings opens the settings sheet, Check for updates reports, Pin toggles, Quit exits.
- `pnpm dev` in a browser runs the panel against the mock backend.
