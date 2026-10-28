# Hopper

A Postman-like companion for internal HTTP APIs, built with Tauri 2 + React. Hopper has no main window: it lives in a small panel docked to a screen edge and in the tray. The AI maintains the request collections through an MCP server; you browse, configure environments and send.

Status: phase 1 (MVP) is implemented and tested: panel, search over OpenAPI specs, request editor and response viewer, environments with keyring secrets, the engine, the MCP server with per-request agent access (Ask / Auto / Blocked) and in-panel approvals. Not yet verified on a real desktop: the panel's Windows behaviours, tray, hotkey and focus handoff (manual checklist in `docs/spikes.md`). The plan is in `docs/PLAN.md`.

MCP: Hopper serves Streamable HTTP on `127.0.0.1` (port from 38517) with a per-install bearer token; the Agent sheet shows the exact `claude mcp add` command.

## Development

Requires Node 24.4.1 (see `.node-version`), pnpm 10, and the Rust toolchain.

```sh
pnpm install
pnpm tauri dev      # the desktop app
pnpm dev            # the web front end alone, against the mock backend in src/ipc/dev
pnpm typecheck
pnpm test
cargo test -p hopper-core   # fast Rust test loop, no Tauri compile
```

## Releases

Releases are built by `.github/workflows/release.yml` when a `vX.Y.Z` tag is pushed: it sets the version from the tag, builds the NSIS installer, signs it, and uploads it with `latest.json` to the GitHub release. The signing key is the `TAURI_SIGNING_PRIVATE_KEY` secret of the `Production` environment; its public half is in `plugins.updater.pubkey` in `src-tauri/tauri.conf.json`. The private key lives at `~/.tauri/hopper.key`: keep a backup, because without it installed copies can't be updated.

The in-app updater is `src/updater.ts` (checks 2 s after launch and every 30 min in a packaged build), shown in `src/sheets/UpdatesSection.tsx` (Settings) and `src/views/UpdateButton.tsx` (header, only while an update needs attention). The tray's "Check for updates" opens Settings and runs a check.

The installer isn't Authenticode-signed, so Windows SmartScreen warns on the first install ("More info" > "Run anyway"). Updates after that are verified by the updater signature.
