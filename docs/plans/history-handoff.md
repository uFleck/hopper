# History, labels, menus, tooltips, updater: handoff

Read `docs/plans/history-labels-and-polish.md` first (the design), then `docs/mockups/history-ui.html` and `docs/mockups/updater.html` (open in a browser; they are the visual spec). This file is only the progress and the next steps.

## Status: all frontend work built (2026-10-30), not committed

Wave 1 and Wave 2 are done: History tab and search scoping, update card, context menu and tooltips, `title=` migration with the vitest guard, Undo all, open a run, row menu, name-tag copy and easter eggs. Checks green: `cargo test -p hopper-core` (325), `cargo check -p hopper`, `pnpm exec tsc --noEmit`, `pnpm vitest run` (177). Seen once in the browser at 640 px (History tab, open a run). Not seen in the packaged app: custom Paste, tooltips, right-click menus, banner states, easter eggs, Undo all with a differing stored request. The sections below are the original plan, kept for reference.

## Done and verified (not committed)

Backend and contract, all checks green: `cargo test -p hopper-core` (325 passed), `cargo check -p hopper`, `pnpm exec tsc --noEmit`, `pnpm vitest run` (155 passed).

- `crates/hopper-core/src/userdata/history.rs`: `label`, `request` (stored request without literal credentials), `request_truncated` on `HistoryEntry`; `history/index.json` (rebuilt when missing or unreadable); `MAX_UNLABELLED = 100` per origin (UI and agent counted apart), labelled runs never pruned; `list_all`, `set_label`, `delete`; `without_literal_credentials`.
- `crates/hopper-core/src/service/`: `HistoryItem` gains `label?`, `hasRequest`; new `HistoryRow` (`name`, `saved`) and `HistoryRequest` (`doc`, `truncated`); `history_all`, `history_label`, `history_delete`, `history_request` in `mod.rs`; sends store the request in `send.rs`.
- `src-tauri/src/ipc/history.rs` and `lib.rs`: four new commands.
- `src/ipc/types.ts`, `src/ipc/contract.test.ts`, `src/ipc/dev/mockBackend.ts` (mock labels, prunes, stores requests, seeds one labelled run), fixtures regenerated (`fixtures/ipc/HistoryItem.json`, `HistoryRow.json`).

## Still to build (frontend only, in two waves)

Rules: Haiku for these, effort low or medium, tool-call cap 15 each, narrow checks only (`pnpm exec tsc --noEmit`, `pnpm vitest run <file>`), no commit, no build. Agents share the working tree, so the file lists below must stay disjoint inside a wave.

### Wave 1 (three agents in parallel)

1. **History tab and search scoping.** Files: `src/store/*` (add `historyAll: HistoryRow[]`, `refreshHistory()` called where `recent` is refreshed, `labelRun`, `deleteRun` with undo toast, `openRun`), new `src/views/History.tsx`, `src/views/Recent.tsx` (add a `query` prop filter), `src/views/FindView.tsx` (tab type `recent | saved | history`, the existing `Segmented`, placeholder per tab, search text filters Recent and History in place and keeps the tab strip visible, All saved keeps today's behaviour, switching tabs clears the text), `src/styles/find.css` or a new `history.css`. Row design, chips, day groups, empty states, inline rename: `history-ui.html`. A filter test file for the matching function (label, name, URL, method, status; all words must match).
2. **Update card.** New `src/views/UpdateBanner.tsx` (+ css), reads `src/updater.ts` like `UpdateButton.tsx`; states available, downloading, ready, error as in `updater.html` U1; "What's new" unfolds notes; move the `Release` renderer from `src/sheets/UpdatesSection.tsx` to a shared file. Do not edit `FindView.tsx`: the main session adds the one line that mounts it.
3. **Context menu and tooltip infrastructure.** New `src/ui/ContextMenu.tsx` (+ `useContextMenu`), split a `MenuList` out of `src/views/find/Menu.tsx`, new `src/ui/Tooltip.tsx` (one delegated host reading `data-tip`, 500 ms delay, themed), export from `src/ui/index.ts`, global `contextmenu` listener and `Tooltip` mount in `src/App.tsx` (prevent default in production builds only, standard Cut/Copy/Paste/Select all menu on inputs, textareas and the CodeMirror editor). Do not migrate `title=` yet.

### Wave 2 (after wave 1 is reviewed; also three agents)

4. **`title=` migration.** The ~69 `title=` attributes become `data-tip=`; `IconButton` and `Chip` in `src/ui/primitives.tsx` forward their label to `data-tip`; add the vitest guard that fails on `title=` on a DOM element (allowlist: `Sheet`, `Section`, `ConfirmDialog`).
5. **Undo all.** `revertDraft` in `src/store/reducers.ts` (doc, collection, folder back to `base`, `dirty` recomputed) with a unit test, `undoAll()` in `src/store/actions.ts`, an "Undo all" button before Save in `src/views/request/RequestHeader.tsx` while `draft.dirty && saved && !readOnly`. Do this together with (4) only if that agent is told to leave `RequestHeader.tsx` alone, otherwise run it first.
6. **Open a run and the row menu.** `openRun`: call `history_request` and `history_get`, put the stored request into the draft as unsaved changes (saved request), or open a new draft (unsaved run) with the "Not saved · opened from history" strip and Save as request; fall back to the saved request plus that run's response when `hasRequest` is false. History row context menu items as in the plan.

After each wave: run the checks, review the combined `git diff`, then run the app through Spawner (`hub_start`) and look at it once on one screen.

## Name tag (Minecraft) copy and easter eggs, requested

A label is a name tag: a labelled run never despawns. Make that visible, lightly:

- Label icon is a name tag. Tooltip on a label: "Name-tagged. This run will not despawn."
- Rename hint line: "Name tag: keeps this run for good" next to Enter and Esc.
- First time a run is labelled in a session, a toast: "Name-tagged. It will not despawn."
- Footer: "87 runs kept · 4 name-tagged never despawn".
- Removing a label: toast "Name tag removed. This run can despawn again."
- Clear all dialog keeps plain wording (it deletes data).
- Easter eggs on the label text only, case-insensitive: `Dinnerbone` or `Grumm` renders the row title upside down (CSS `rotate(180deg)`), `jeb_` cycles the label colour (hue animation, off under `prefers-reduced-motion`).
- Keep the rest calm, as with the updater.

## Open points

- Paste in the custom text menu needs `navigator.clipboard.readText()` to work in the packaged WebView2 app. If it does not, add `tauri-plugin-clipboard-manager` and grant it in capabilities.
- Nothing is committed. Release notes for the next version should mention history with labels, custom menus and tooltips, and the update card.
