# History with labels, custom menus and tooltips, prominent updater

Mockups: `docs/mockups/history-ui.html` (the chosen history design), `docs/mockups/history.html` (the earlier options), `docs/mockups/updater.html` (update prompt).

## Decisions

- **History is a third home tab**: Recent | All saved | History. Recent stays as it is.
- **The search box follows the active tab.** History searches history, Recent filters recent, All saved keeps today's saved plus spec-route search.
- **Rename is a label on one run.** It never changes the saved request's name. Gestures: inline (double-click, F2, pencil on hover) plus a right-click menu.
- **Keep the newest 100 unlabelled runs**; labelled runs never expire. One constant (`MAX_UNLABELLED`). The cap counts UI sends and agent sends separately.
- **Agent sends are hidden by default**, shown through an "Agent" chip.
- **Each run stores the request that was sent** (method, URL, headers, body, auth template, before variables are resolved), so a run can be reopened and re-sent exactly. Opening a run loads that request into the editor as unsaved changes.
- **Undo all**: a button next to Save whenever a saved request has unsaved changes. It returns the editor to the latest saved version.
- Right-click never shows the browser menu. Hover never shows the browser's native tooltip.
- The updater is a quiet card on the home screen (`updater.html`, U1).

## 1. History

### What exists today

- `userdata/history.rs` stores one JSON file per run at `history/<requestId>/<ulid>.json`, keeps the newest 20 per request, and has no way to list across requests.
- Unsaved sends are already stored, under the request id `inline`, all in one 20-entry folder.
- `HistoryEntry` has a masked URL and headers, the response, and no label and no request body.
- Recent (`recent_list`) is the ids of the last saved requests used, not a history.

### Storage (`hopper-core`, Sonnet writes this)

- `HistoryEntry` gains `label: Option<String>` and `request: Option<Request>` (both `#[serde(default, skip_serializing_if = "Option::is_none")]`, so old files still load).
- `request` is `snap.request` from `service/send.rs`, i.e. the document before variable resolution: `{{variables}}` stay as written, no resolved secret values are in it. Literal credentials typed into Auth (bearer token, basic password, API key value) are blanked unless they are a `{{...}}` template, because history files are not part of the collection. Headers are kept as written (the masked copy is already stored today). If the JSON is over 256 KB the body is dropped and `request` is stored without it, flagged so the UI says so.
- New `history/index.json`: one small row per run (id, requestId, sentAt, method, url, status, totalMs, sizeBytes, env, via, label, hasRequest). Listing reads only this file, never the bodies (a body can be 1 MiB). If it is missing or fails to parse it is rebuilt by scanning the folders. All changes happen under the existing history lock.
- `add` appends to the index, then prunes: unlabelled runs beyond `MAX_UNLABELLED` are deleted (file and index row), counted separately for `Via::Ui` and `Via::Mcp`. Labelled runs are never pruned. This replaces the per-request 20 cap; the request page's list is the index filtered by `requestId`.
- New methods: `list_all()`, `set_label(request_id, id, Option<String>)` (trim, empty becomes `None`, max 80 chars), `delete(request_id, id)`.
- Deleting a saved request keeps its history, as today. Rows whose request no longer exists show as unsaved.

### Service and IPC (Sonnet writes the contract)

- `HistoryItem` gains `label?: string` and `hasRequest: boolean`. New `HistoryRow = HistoryItem & { name: string; saved: boolean }`: `name` is the saved request's name, or the URL host and path for unsaved runs. The service fills it from the store, like `recent_list`.
- New commands: `history_all` (`HistoryRow[]`), `history_label { requestId, id, label | null }`, `history_delete { requestId, id }`, and `history_request { requestId, id }` returning the stored request. `history_get` (the response) and `history_clear` stay.
- `src/ipc/types.ts`, the dev mock backend (`src/ipc/dev/mockBackend.ts`) and `contract.test.ts` change together. MCP `hopper_list_history` returns `label` too.
- Policy stays in the service. The IPC commands are thin.

### Frontend (Haiku once the contract exists)

- `FindView` tab type becomes `"recent" | "saved" | "history"`, in the existing `Segmented` control: `Recent`, `All saved · N`, `History · N`. The tab strip also shows when History or Recent is searched; today it hides while searching.
- **Dynamic search.** The box's placeholder and behaviour come from the tab: History "Search history", Recent "Search recent", All saved "Find a request or route" (unchanged). History and Recent filter their list in place, client-side, with the tab strip and chips staying. Typing never switches tabs. Switching tabs clears the text. Facets (method, tag) stay saved-search only.
- History search matches label, request name, URL, method and status, all words must match, with highlighting.
- `Recent.tsx` is unchanged except for the filter. New `History.tsx`. Store gets `historyAll: HistoryRow[]`, refreshed where `recent` is refreshed (after a send, after agent activity).
- Rows: method chip, title (label in the accent chip, otherwise the request name), second line (request name and path when labelled, else path), status chip, relative time, `UNSAVED` and `AGENT` tags. Day groups: Today, Yesterday, Last week, then month. Chips: All, Named, Failed, Unsaved, Agent (off by default).
- Footer: "N runs kept · 100 max" and "Clear all" (in-app `confirm`, wording says labelled runs go too).
- Rename: inline edit on the title, Enter saves, Esc cancels, empty removes the label. Optimistic, rolled back with a toast on failure.
- Delete from history is instant; a toast offers Undo (the row leaves the index at once, the file is removed when the toast expires). Empty states: "Nothing sent yet" and "No matches".

### Opening a run

- Click calls `history_request` and `history_get`. For a saved request: open it, set the draft's `doc` to the stored request (this makes it dirty against `base`), and show the run's response in the response pane. The response-history viewer that already exists is reused for the response.
- For an unsaved run: open a new draft from the stored request, with a strip "Not saved · opened from history" and a Save as request button. There is no saved version, so no Undo all.
- If a run has no stored request (older runs, or a dropped body), it opens as today: the saved request as it is now, plus that run's response, and the strip says the request could not be restored.

### Undo all

- New pure function `revertDraft(d)` in `src/store/reducers.ts`: `doc`, `collection` and `folder` go back to `base`, `dirty` is recomputed. New action `undoAll()` in `actions.ts`.
- `RequestHeader` shows an "Undo all" button before Save while `draft.dirty && saved` (hidden for never-saved drafts and read-only requests). The tooltip says it discards all changes and goes back to the saved version. It confirms nothing; the result is just the saved state.

## 2. Custom context menu everywhere

- New `ui/ContextMenu.tsx` plus `useContextMenu(items)`. It reuses the list rendering of `views/find/Menu.tsx`, split out as a `MenuList` that takes a point instead of a trigger button, with the same keyboard handling and Esc behaviour.
- One global `contextmenu` listener in `App.tsx`: if nothing handled the event it calls `preventDefault()`, so the browser menu never appears. Dev builds let it through so the inspector still works (`import.meta.env.PROD`, as the updater does).
- Editable targets (inputs, textareas, the CodeMirror editor) get a Cut, Copy, Paste, Select all menu. Paste needs `navigator.clipboard.readText()`, which must be tried in the packaged WebView2 app. If it is refused, add `tauri-plugin-clipboard-manager` and grant it in capabilities.
- History rows: Rename or Add label (F2), Remove label (when labelled), Open request or Open as draft, Save as request (unsaved only), Delete from history. Elsewhere a right-click does nothing.

## 3. No native tooltips

- 69 `title=` attributes in about 30 files. The native tooltip comes from the `title` attribute, so the fix is to stop using it.
- New `ui/Tooltip` host: one delegated listener reading `data-tip`, 500 ms delay, fixed position, themed with the existing tokens, hidden on press, scroll and Esc.
- `IconButton` and `Chip` (`ui/primitives.tsx`) forward their label to `data-tip` instead of `title`. `aria-label` stays.
- Mechanical migration of the other `title=` uses to `data-tip=`.
- Guard so it stays fixed: a vitest test scanning `src/**/*.tsx` that fails on a `title=` attribute on a DOM element, with an allowlist for components whose `title` prop is not a DOM attribute (`Sheet`, `Section`, `ConfirmDialog`). `useHoverCard` is untouched.

## 4. Updater on the home screen

- New `views/UpdateBanner.tsx` at the top of `FindView`, reading `src/updater.ts` through `useSyncExternalStore` like `UpdateButton`. `updater.ts` stays the single source of update state.
- States and the one button: available (Update), downloading (2 px progress line), ready (green edge, Restart), error (red edge, Try again). "Update" calls `download()`; when state becomes `ready` the card turns green and waits for Restart.
- "What's new" unfolds `upd.notes` under the card. The `Release` renderer in `UpdatesSection.tsx` moves to a shared file.
- The header icon stays (it is on every screen) and keeps its dot. Settings > Updates keeps the long view.
- Calm by design: a normal panel card with a 3 px accent edge. No glow, shine or bounce.

## 5. Tests

- `cargo test -p hopper-core`: index rebuild when missing or corrupt; label set, trim, empty, over-length; cap counted per `Via`; labelled survives pruning; old entry files without `label` or `request` still load; stored request keeps `{{variables}}`, blanks literal credentials, drops an oversize body; delete.
- `src/store/store.test.ts` and `src/ipc/contract.test.ts`: new commands, `historyAll`, `revertDraft` (dirty to clean, never-saved draft untouched), opening a run sets the draft dirty.
- Vitest: the `title=` guard; history search matching (label, name, URL, method, status).
- Run through Spawner on one screen: History tab, label, rename, chips, search scoping per tab, open a run then Undo all, context menu, banner states.

## 6. Order and execution

1. Storage, service, IPC types and contract (section 1, backend).
2. Then, in parallel on separate files: History tab and search scoping, Undo all, context menu, tooltips with the `title=` migration, update banner.

Execution: Split. Sonnet writes the `hopper-core` storage and service, the IPC contract and the shared types (a contract used by Rust and TypeScript, and it decides what gets persisted). Haiku builds the consumers (`History.tsx` and search scoping, `revertDraft`/Undo all, `ContextMenu`, `Tooltip` plus the `title=` migration, `UpdateBanner`), each with a narrow check (`cargo test -p hopper-core`, `pnpm vitest run <file>`, `pnpm tsc --noEmit`). Sonnet reviews the single combined diff.

## Notes

- Literal credentials are blanked in stored requests, but a secret typed into a header or the body is stored as written, as it already is in the request file and today's masked history headers.
- Cost to build: about 5 Haiku tasks at roughly 15 calls each after one Sonnet backend step, well under 10% of a 5h window by the measured rule.
