# Hopper: plan

## Context

Calling internal HTTP APIs by hand means remembering the base URL, digging the API key out of Bitwarden, and finding the endpoint in a Swagger UI that handles large apps badly (many `User*` controllers). Today an AI finds the route and writes a cURL. Hopper is a Postman-like desktop client where the **AI maintains the collections through an MCP server** and the human browses, configures environments, and sends. It will be given to coworkers, so install friction and secret safety matter more than feature count.

Decisions already made (not revisited): **Hopper is a companion, not a full application (section 0)**; separate Tauri 2 app; reqwest in Rust, no Bruno at runtime; collections are plain files we own, no secrets inside; secrets are references behind a provider interface (OS credential store default; Bitwarden deferred); MCP server; Windows first; Spawner-style releases; no native dialogs except the folder picker.

Answers to the open questions (final): public GitHub repo (`uFleck/hopper`); installer stays unsigned (SmartScreen "Run anyway" documented); workspace placement is flexible (per-repo or central, app remembers several recent workspaces); **no Bitwarden integration for now** (the `SecretProvider` trait stays so it can be added later, but nothing `bw`-specific is built); the AI never gets blanket access outside `local`: the user decides **per request** whether the agent may run it on its own (Ask / Auto / Blocked), and `local` environments are always allowed (section 4).

What I verified in Spawner (`C:\Projects\spawner`): Node `24.4.1`; Rust edition 2024; React 19, Vite 8, TS 6, pnpm 10; plain CSS with `src/styles/tokens.css` (dark tokens, `--r: 4px`, Inter + JetBrains Mono + Silkscreen, Phosphor icons, "Panes" skin); a hand-rolled MCP server in `src-tauri/src/mcp.rs` (axum, Streamable HTTP, JSON responses only, loopback, port probing from 38417 saved in `mcp-port.txt`); `.github/workflows/release.yml` (tag -> version written into `tauri.conf.json`/`Cargo.toml`/`package.json` at build only -> `tauri-action` with `uploadUpdaterJson`); a companion window (`companion.rs`, `src/companion/*`); `src/dev/mockBackend.ts` for browser dev. Hopper copies these conventions instead of reinventing them.

---

## 0. Product shape: a companion, not a window

Hopper follows the philosophy of Spawner's companion (`src-tauri/src/companion.rs`, `src/companion/*`): a small always-on-top panel docked to a screen edge. There is **no main window, no fullscreen mode, no taskbar button, and no Alt+Tab entry**. The goal is fewer context switches, so every feature (search, requests, responses, environments and secrets, agent grants and approvals, MCP setup, settings, updates) lives inside the panel.

### Window and states

- Exactly one window, built in Rust at startup (`app.windows: []` in `tauri.conf.json`), the way Spawner builds its companion: undecorated, transparent, always on top, `skip_taskbar`, hidden from Alt+Tab (tool-window style), shown without activation (`SW_SHOWNOACTIVATE`).
- Four states, stuck to the left or right screen edge as in the Circuit mockup (`docs/redesign/b-circuit.html`): **hidden**, **tab** (a handle flush with the edge, half out of sight until hovered; drag it along the edge, or to the other edge, and its position is remembered in `dock.json`), **compact** (a pill 16 px in at the top corner: a new endpoint an agent added over MCP, a pending approval, or what is running; also the pinned ticker), **open** (the full panel, 430 px wide, 16 px from the edge, top and bottom). Dragging the open panel by its mascot snaps it to the nearest edge on release. The handle stays on screen by default; "Always show the edge handle" can be turned off to hide it when idle. There is no width switcher.
- Docking: left or right edge of any monitor, any height, remembered; drag the mascot to move it and it snaps to the nearer edge (Spawner's `place.ts` anchor logic). Width presets **430 / 640 / 860 px** plus a drag handle on the inner edge, remembered; height up to the work area.
- Tray icon is the escape hatch and the only way to quit: Show/Hide, Pin, Settings, Check for updates, Start with Windows (on by default after the first run, like Spawner), Quit. Single instance.

### How it appears (this is the part that removes Alt+Tab)

- **Summon (active):** a global hotkey (default `Ctrl+Alt+H`, configurable, via `tauri-plugin-global-shortcut`; if taken, the settings say so and the tray still works) opens the panel, **takes focus, and remembers the window that had it**. The search box is focused. Flow: hotkey, type, Enter, Send, copy the response, Esc. Esc, the hotkey again, or clicking away (when not pinned) closes it and **hands focus back to the previous window**, using Spawner's `PREVIOUS` handoff in `set_focus`.
- **Passive show:** agent activity, a request in flight, or a finished response shows the panel **without taking focus**, so keystrokes meant for another window are never captured. The mascot reflects the state: sleeping, sending, agent working, **needs approval**, error.
- **Approvals never steal keystrokes:** the in-panel approval prompt has no default button, ignores the keyboard for its first ~500 ms, and needs a click (or an explicit keyboard focus by the user). If the panel was hidden, it appears passively and raises an OS notification.
- **Hover** over the mascot opens the panel; the backend polls the pointer position like Spawner's `companion_pointer` because clicking inside moves keyboard focus away from the page and the webview then misreports the pointer.

### What the shape changes in the rest of the plan

- No multi-pane app layout and no tab bar: one vertical column with progressive disclosure, and a **Recent** strip instead of tabs.
- No custom title bar or `Ctrl+K` palette: the search box *is* the home screen.
- No second window anywhere. Settings, environments, agent and approvals are **sheets inside the panel**; the native folder picker stays the only native dialog.
- Everything must be usable at 430 px, and gets better at 640 or 860.

### Reuse from Spawner (copy now, extract a shared crate later if a third app appears)

`companion.rs` window builder, `set_visible`, `place` (`SetWindowPos` with no activation), `drag` (move loop inside the call), `pointer`, `set_focus` with the `PREVIOUS` window handoff, `hide_from_alt_tab`; `place.ts` anchors per monitor; `view.ts` shared view state in localStorage; the `companion` capability file (minimal permissions). **Not ported:** Spawner's hidden Minecraft skin and any art from it; Hopper's mascot is original.

---

## 1. Stack (confirmed, with two upgrades)

| Layer | Choice | Note |
| --- | --- | --- |
| Shell | Tauri 2, Rust 2024 | plugins: updater, process, dialog (folder picker only), opener, single-instance, notification, autostart, **global-shortcut**; `tray-icon` (required: there is no main window); `windows-sys` for the panel's Win32 behaviors |
| Frontend | React 19 + TS + Vite, pnpm 10, Node 24.4.1 | plain CSS + tokens like Spawner, tiny hand-rolled store like Spawner's `store.ts`, **CodeMirror 6** for body/response editors (Monaco is too heavy) |
| HTTP | `reqwest` 0.13 | **upgrade**: 0.13 defaults to rustls + `rustls-platform-verifier` (OS trust store). Spike must prove an internal-CA host works on Windows; fallback is the `native-tls` backend (SChannel). Features: gzip, brotli, deflate, zstd, cookies, multipart, stream, system proxy |
| MCP | `rmcp` (official Rust SDK) over axum 0.8, Streamable HTTP | **upgrade, gated by a spike**: typed tools with `schemars` schemas shared with core types. If it fights the Tauri tokio runtime, fall back to Spawner's hand-rolled `mcp.rs` pattern (proven) |
| Files | YAML via a maintained serde fork (`serde_yaml` is archived; pick `serde_yaml_ng` or similar in the spike) | canonical writer, unknown keys preserved |
| Secrets | `keyring` crate, `secrecy`/`zeroize` | `keyring` is the only provider in the MVP; the provider trait leaves room for others later |
| Search | `nucleo-matcher` over an in-memory index | runs in Rust so UI palette and MCP share one ranking |
| Watching | `notify` | external edits (git pull, AI editing files directly) reload live |
| IPC types | generate TS from Rust (`tauri-specta`, or `ts-rs` if specta is not stable on Tauri 2 at scaffold time) | decided in the spike; CI fails if generated types are stale |

**Cargo workspace, two crates**: `crates/hopper-core` (pure Rust: format, vars, secrets, openapi, search, engine, policy; no Tauri dependency) and `src-tauri` (Tauri shell, IPC, MCP adapter). Reason: `cargo test -p hopper-core` runs in seconds without compiling Tauri, and a future CLI or headless MCP reuses core. Spawner is a single crate; this is the one structural departure and it pays for itself in test speed.

---

## 2. Domain model and file format

### Concepts

- **Workspace**: a folder the user points Hopper at (can be inside a repo). Contains collections and environments. Has a stable random `id` that namespaces keyring entries.
- **Collection**: one API/app (e.g. "Account API"). Holds shared headers/auth, linked OpenAPI specs, folders, requests.
- **Folder**: optional grouping with its own inheritable auth/headers and an explicit order.
- **Request**: method, URL with `{{variables}}`, path params, query, headers, body, auth, notes, optional `source` link to an OpenAPI operation.
- **Environment**: plain variables, secret references, TLS/proxy flags, kind (local/dev/staging/production/other).
- **Response history**: local only, never in the workspace folder.

### On disk

```
my-apis/                          (the workspace; may be a git repo)
  hopper.yaml
  .gitignore                      (created by Hopper: *.local.yaml)
  environments/
    local.env.yaml
    staging.env.yaml
    staging.local.yaml            (per-user override, gitignored, e.g. local base URL)
    production.env.yaml
  collections/
    account-api/
      collection.yaml
      users/
        folder.yaml
        get-user-by-id.req.yaml
        search-users.req.yaml
```

History and the spec snapshot cache live in the app data dir (`%APPDATA%\dev.hopper.app\workspaces\<workspaceId>\`), so the repo is never polluted and bodies never get committed by accident.

Every file starts with `hopper: <int>` and a `kind`. Requests are referenced by a stable ULID `id`, not by path, so renames and moves keep history, links and MCP references intact.

`hopper.yaml`
```yaml
hopper: 1
kind: workspace
id: 7c1e9a52-3b0e-4f6a-9d0e-1b8f5a2c4e77
name: Umbler APIs
defaults:
  timeoutMs: 30000
  followRedirects: true
```

`collections/account-api/collection.yaml`
```yaml
hopper: 1
kind: collection
name: Account API
headers:
  - { name: X-Tenant, value: "{{tenantId}}" }
auth: { type: bearer, token: "{{apiKey}}" }      # values are variable references only
specs:
  - name: account
    url: "{{baseUrl}}/swagger/v1/swagger.json"   # resolved with the active environment
    importedAt: 2026-10-26T14:02:00Z
    docHash: sha256:9f2c...                       # of the normalized document
```

`collections/account-api/users/folder.yaml`
```yaml
hopper: 1
kind: folder
name: Users
order: [get-user-by-id, search-users]   # optional; default is alphabetical, which merges cleanly in git
auth: { type: inherit }
```

`collections/account-api/users/get-user-by-id.req.yaml`
```yaml
hopper: 1
kind: request
id: 01JBX7Q4M8K3Z2N5V6T9R0YHAD
name: Get user by id
method: GET
url: "{{baseUrl}}/api/users/{id}"          # {{var}} = Hopper variable, {id} = OpenAPI path param
pathParams:
  id: "{{lastUserId}}"
query:
  - { name: includeRoles, value: "true", enabled: true }
  - { name: expand, value: "", enabled: false }     # known from the spec, off by default
headers:
  - { name: Accept, value: application/json, enabled: true }
body: { type: none }                       # none | json | text | xml | form | multipart | binary
auth: { type: inherit }                    # inherit | none | bearer | basic | apiKey
notes: |
  Returns 404 for soft-deleted users.
source:                                    # present when born from an OpenAPI operation
  spec: account
  key: GetUserById                         # operationId, else "GET /api/users/{id}"
  hash: sha256:51ab...                     # hash of the normalized operation at import
  orphaned: false                          # set when a re-import no longer finds it
```

`environments/staging.env.yaml`
```yaml
hopper: 1
kind: environment
name: staging
# stage file: environments/<stage>.env.yaml, stage = local | dev | rc | production (no envKind)
variables:
  baseUrl: https://account.staging.example.com
  tenantId: "42"
secrets:
  apiKey:     { provider: keyring }        # key derived: hopper/<workspaceId>/staging/apiKey
  adminToken: { provider: keyring }
tls: { insecureSkipVerify: false }
allowedHosts: [account.staging.example.com]   # optional; default = hosts of variables ending in baseUrl/Url
```

Response history entry (app data, `history/<requestId>/<timestamp>.json`, last 20 per request)
```json
{ "id": "01JBX...", "requestId": "01JBX7Q4...", "env": "staging", "via": "ui",
  "sentAt": "2026-10-26T14:05:11Z", "method": "GET",
  "url": "https://account.staging.example.com/api/users/42?includeRoles=true",
  "requestHeaders": [["Authorization", "Bearer ••••"]],
  "status": 200, "timing": { "ttfbMs": 148, "totalMs": 171 }, "sizeBytes": 812, "decodedBytes": 2310,
  "responseHeaders": [["content-type", "application/json"]],
  "bodyTruncated": false, "body": "{...}" }
```
Resolved secret values are replaced by `••••` before anything is stored (the resolver tracks which segments came from secrets). Bodies capped at 1 MB in history. "Clear history" in the UI.

### Rules that keep git diffs clean when an AI writes files

- One small file per request/environment; alphabetical default order; no per-file sequence numbers.
- Canonical writer: fixed key order, 2-space indent, LF, trailing newline. Comments in YAML are not preserved (documented); `notes` is the place for prose.
- Atomic writes (temp file + rename). Every file has a content-hash **revision**; MCP updates carry `expectedRevision` so an agent cannot silently overwrite an edit the user just made.

### Versioning and migration

- `hopper: N` is an integer per file, not semver. Additive optional fields do not bump it; unknown keys are kept in an `extra` map and written back, so an older Hopper does not destroy data from a newer one.
- A file with N greater than the app supports opens **read-only** with a banner ("update Hopper"); it is never rewritten.
- Older files are migrated **in memory** through a chain of pure functions `migrate_vN_to_vN+1(Value) -> Value` before typed deserialization. They are rewritten only on explicit save or via a "Upgrade workspace" action, so a Hopper update never produces a surprise 200-file diff.
- Frozen golden fixtures per version under `fixtures/format/vN/`; a test migrates every old fixture to current and snapshot-compares. JSON Schemas generated from the Rust types (`schemas/`) are shipped to editors and returned by an MCP `hopper_describe_format` tool so agents write valid files.

---

## 3. OpenAPI import and route search (the differentiator)

### Model: a spec index separate from requests, materialized lazily

Importing a spec does **not** create hundreds of request files. It builds a searchable **operation index** (kept in memory and cached in app data); a request file is created only when someone adds an operation to the collection (the user clicks it, or the AI calls `hopper_create_request` with `fromOperation`). Opening an operation from search creates an unsaved draft in a tab; saving promotes it to a file with a `source` link. Result: a 2,000-operation API stays browsable and searchable without 2,000 files in the repo.

### Ingest

- Source: URL (resolved with the active environment, using that environment's TLS/proxy/auth, since many specs are behind auth) or local file. Detect and offer multiple documents from Swagger UI config (`urls`).
- Versions: Swagger 2.0, OpenAPI 3.0, 3.1. Parse as `serde_json::Value` with our own extractor and `$ref` resolver (the typed `openapiv3` crate handles 3.0 only). Extract per operation: method, path, `operationId`, summary, description, tags, parameters (path/query/header/cookie), request body media types with a depth- and cycle-limited example generated from the schema, security hints, deprecated flag, server URLs.
- Large specs: parse and index in `spawn_blocking`, show progress, target under 1 s for a 5 MB document. A synthetic 3,000-operation fixture is part of the tests.
- Duplicate or missing `operationId` (common with Swashbuckle) falls back to `METHOD path` as the key.

### Search

- Indexed fields with weights: path segments (also camelCase/PascalCase split), `operationId`, tag/controller, summary, description (low weight), method. Matching with `nucleo`; all query tokens must match somewhere (AND), each token fuzzy.
- Query syntax: free words (`get user roles`); method prefix (`post users`); `tag:UserAdmin`; `#GetUserById` for operationId; `path:/api/users/{id}`.
- Ranking: exact segment match > prefix > fuzzy; saved requests and recently used get a boost; deprecated operations sink.
- This is what fixes the "many `User*` controllers" problem: results show the tag/controller as a chip next to a path with the matched characters highlighted, so `UserAdminController` vs `UserProfileController` is visible at a glance; a facet row narrows by tag and method; the sidebar has a "Spec" tab that browses by tag as a fallback.
- One implementation in `hopper-core::search`, used by the palette (IPC) and by the MCP `hopper_search_routes` tool.

### Re-import and drift

Operation identity: `operationId` if unique, else `METHOD path`. Re-import compares against the **new** document using the `source.hash` stored in each request file, so no old snapshot has to be kept or shared between coworkers. Per operation the diff reports: unchanged, **changed** (hash differs), **new** (not in the collection, offered, never auto-added), **removed** (request gets `source.orphaned: true`, never deleted), and **moved** (same `operationId`, new path: URL updated).

What gets rewritten on a changed operation is limited to **spec-owned structure**: method, path template, and the *set of parameter names*. Merge by parameter name; **values, headers, body content, auth, notes are never overwritten**. New parameters appear as disabled rows; parameters removed from the spec are flagged "no longer in spec" but kept. The UI shows a per-request diff before applying; the MCP tool `hopper_reimport_spec` has a `dryRun` default.

---

## 4. MCP server

### Transport and shared state

- **Local Streamable HTTP, loopback only, served by the running app** (same approach as Spawner). The app process owns the workspace; the file on disk is the persistent truth, the in-memory `WorkspaceService` the runtime truth, and both the UI (Tauri IPC) and MCP call the **same service methods**. No second process reads or writes the files, so there is nothing to reconcile.
- Rejected for MVP: stdio server as its own process. It would be a second writer and would need to find the running app anyway. A thin `hopper-mcp` stdio shim that proxies to the HTTP port can be added later for agents without HTTP support.
- Port: probe from 38517 (not Spawner's 38417 range), persist in `mcp-port.txt`.
- **Hardening (new vs Spawner, because `send` plus secrets is behind this port):** reject requests whose `Origin` is a browser origin and validate `Host` (blocks drive-by web pages and DNS rebinding); require a per-install bearer token generated at first run. The in-app setup guide runs `claude mcp add --transport http hopper <url> --header "Authorization: Bearer <token>"` for the user and offers a copy-paste config for other agents.
- When Hopper is not running the agent gets a connection error; the tool instructions tell it to ask the user to open Hopper. The tray keeps it alive.

### Tools (JSON results, every output size-capped)

| Group | Tool | Purpose |
| --- | --- | --- |
| Orient | `hopper_workspace` | workspace, collections, environments (names), specs, current active env, format version, usage hints |
| | `hopper_describe_format` | JSON Schemas and rules for request/environment files |
| Find | `hopper_search_routes` | fuzzy search over spec operations and saved requests (`query`, `collection`, `tag`, `method`, `limit`) |
| | `hopper_get_operation` | full operation details: params, body schema and generated example, responses |
| | `hopper_list_requests`, `hopper_get_request` | saved requests by collection/folder/query; one request with `revision` |
| Write | `hopper_import_spec`, `hopper_reimport_spec` | add or refresh a spec (`dryRun` default for re-import) |
| | `hopper_create_request` | from `fromOperation` (preferred) or explicit fields; returns id and path |
| | `hopper_update_request` (`expectedRevision`), `hopper_move_request`, `hopper_create_folder` | structured patches; invalid files are rejected with schema errors |
| | `hopper_delete_request` | moves to an in-app trash with an undo toast (like Spawner's `UndoToasts`) |
| | `hopper_restore_request`, `hopper_duplicate_request` | restore a trashed request by its undo token; copy into the same folder as `<name> copy` (no grant) |
| | `hopper_create_collection`, `hopper_get_collection`, `hopper_update_collection` (`expectedRevision`) | rename a collection or unlink spec links; collection headers and auth are never shown or changed by agents, since they can hold literal credentials |
| | `hopper_delete_collection`, `hopper_delete_folder` | only when empty (no requests, folders or spec links), so nothing is lost |
| | `hopper_upsert_environment` | create an environment, add/change variables, **declare secret references**; cannot set secret values, cannot alter `tls` or `allowedHosts` of an existing application section |
| Environments | `hopper_list_environments`, `hopper_get_environment` | non-secret values shown; secrets shown as `{provider, status: set | missing}`, never a value |
| | `hopper_delete_environment`, `hopper_set_active_environment` | removes the environment file (its keyring values stay with the user); sets the active environment or clears it with `null` |
| Run | `hopper_send_request` | by request `id` or inline definition, with `environment` and optional variable overrides; returns status, redacted headers, timing, size, body truncated to ~30k chars plus a `responseId` |
| | `hopper_get_response` | page through a stored response (`offset`, optional JSON path) |
| | `hopper_list_history` | recent runs of a request |
| UI | `hopper_open_in_app` | focus a request in the window so the user sees what the agent built |

### Permission model

Enforced **once, in the services** (every call carries `Caller::User` or `Caller::Agent(client)`), not separately in the MCP adapter and the IPC layer.

- **The user decides, request by request, what the agent may run on its own.** Every saved request has an **agent access** setting with three values: **Ask** (default: the agent may try, the user is prompted), **Auto** (the agent may run it without a prompt) and **Blocked** (the agent can never send it). It is a visible control in the request editor header (a small chip: Ask / Auto / Blocked) and a marker in the sidebar tree, and can be set in bulk (multi-select, or "Auto for all GET requests in this folder", which simply writes per-request grants).
- **Where it is stored:** user-local, in app data keyed by workspace and request `id`, **never in the workspace files and not writable through MCP**. Otherwise an agent (or a coworker's committed file) could grant itself access. Consequence: a freshly cloned workspace starts at Ask for everything, and every coworker chooses their own grants.
- **Grants are bound to a fingerprint** (hash of method, URL template, path params, query, headers, body, auth). If the request is edited **through MCP** the grant is suspended and it asks again until the user reviews and re-confirms ("Changed by the agent, review and re-allow"); an edit made by the user in the UI keeps the grant (saving is the user's consent). This stops "get it allowed, then rewrite it into a DELETE".
- **Environment rules** (user-local, per environment):
  - **`local` environments: always allowed.** The agent may run any request, saved or inline, with no prompt; per-request settings and pattern rules are not consulted. The remaining safeguards (host binding, redaction, rate limit, activity log) still apply.
  - **All other environments** (dev, staging, production, unknown): the decision comes from the request's agent access. **Blocked** wins; **Auto** runs; **Ask** prompts. An environment can additionally be set to `deny` (the agent cannot send there at all). Production is never blanket-allowed: there is no "allow everything" switch for it, only per-request Auto grants.
  - **Unsaved (inline) requests** the agent builds on the fly have no grant, so they **Ask**, unless an optional pattern rule matches (`GET /api/users/*` on an allowed host, matched against the resolved request). A "safe reads" convenience rule (GET/HEAD/OPTIONS on allowed hosts) is available per environment, pre-enabled for dev and staging, never pre-enabled for production.
- `Ask` shows an in-app dialog (never native) with the fully resolved request, secrets masked: Allow once / Allow for this session / **Always allow this request** (sets Auto) / **Block this request** / Deny. The MCP call waits up to 60 s, then returns "not approved". If the window is in the background it raises an OS notification (Spawner already uses the notification plugin).
- Every auto-approved call is logged as "allowed by: local environment | request grant | pattern rule", and the Agent screen lists all grants with a one-click revoke.
- **Secret host binding**: a secret is only substituted into a request whose host is in the environment's `allowedHosts` (default: hosts of its base URL variables). A request to any other host with secrets attached is blocked or forced to `ask`. This is the defence against prompt injection from spec or response contents trying to send the API key to another host.
- Output hygiene: resolved secret values are scrubbed from anything returned to the agent (request echo and response body/headers); auth headers are masked.
- Visibility: every agent call (tool, target, outcome) appears in an **Agent activity** panel; runs are tagged `via MCP` in history; simple rate limit on sends (e.g. 30/min).
- Production extras for the UI too: window chrome tints when a production environment is active, and non-GET sends to a production environment ask for an in-app confirmation (setting per environment).
- Honest limit: GET can have side effects; classification is by method only and documented as such.

---

## 5. Request engine (`hopper-core::engine`)

- **Client**: cache of `reqwest::Client` keyed by (tls-insecure, proxy, cookies on/off, redirect mode). rustls with the platform verifier (OS trust store, so internal CAs work); per-environment `insecureSkipVerify` via `danger_accept_invalid_certs`, shown as a red "TLS verification off" badge on the request and response whenever active. System proxy honored; PAC scripts and NTLM proxies are not supported (risk, section 10). Compression decoding on; user agent `Hopper/<version>`.
- **Timeouts**: connect 10 s, total 30 s default; override per request and per workspace.
- **Redirects**: follow up to 10 by default, per-request toggle; custom policy records the chain (status + URL per hop) for the Timeline tab; Authorization dropped on cross-origin hops (reqwest default).
- **Cookies**: per (workspace, environment) jar, on by default, visible and clearable in a Cookies tab.
- **Auth, in order**: none, bearer, basic, API key (header or query), inherit (request > folder > collection). Next: OAuth2 client credentials.
- **Bodies, in order**: none, JSON (validated, pretty-print action), text, XML, form-urlencoded, multipart (text and file parts), raw binary file. `Content-Type` auto-set unless the user sets one. GraphQL later.
- **Variables**: `{{name}}` resolved at send time in URL, headers, query, body, auth; plus built-ins `{{$guid}}`, `{{$timestamp}}`, `{{$isoTimestamp}}`, `{{$randomInt}}`. Precedence: per-send overrides > environment (vars and secrets) > collection variables > workspace defaults. Unresolved variables block the send with a precise message, not a silent empty string.
- **Timing**: MVP reports TTFB (time to headers), body download time and total. DNS/connect/TLS breakdown needs a custom hyper connector; deferred and called out as such in the UI rather than faked.
- **Cancellation**: each send gets a `runId` and a `CancellationToken`; `tokio::select!` over the request and the body stream; IPC `request_cancel(runId)`; MCP calls are cancelled when the client disconnects.
- **Response handling**: stream the body with a size cap (default 20 MB kept, flag `truncated`, offer "save to file"); sniff type from `Content-Type` and bytes.
- **Rendering**: JSON pretty-printed with folding (CodeMirror), Raw, Preview (HTML/image), headers table, cookies, timeline, status chip, time, size shown as transferred vs decoded; in-response search; large bodies fall back to a virtualized plain view.
- **Copy as cURL**: two actions, masked (safe to paste into chat) and resolved (explicit, because this is the user's current workflow).

---

## 6. Secrets and environments UX

### Resolution

Environments hold **references**. At send time a `SecretProvider` is asked for each referenced value; the value lives in a `SecretString`, is injected into the outgoing request in Rust, and is dropped. The frontend never receives secret values; the only exception is an explicit "Reveal" on a keyring secret (in-app click, auto-hides after 10 s).

```rust
#[async_trait]
trait SecretProvider {
    fn id(&self) -> &'static str;                         // "keyring" in the MVP
    async fn status(&self) -> ProviderStatus;             // ready | locked | unavailable(reason)
    async fn get(&self, r: &SecretRef) -> Result<SecretString, SecretError>;
    async fn set(&self, r: &SecretRef, v: SecretString) -> Result<(), SecretError>; // keyring only at first
}
```
Providers: `keyring` only in the MVP. `env` (read a process env var; cheap, useful for CI and scripts) is a later option (phase 3). Bitwarden, 1Password and others are explicitly **out of scope for now**; they would plug in behind the same trait. Because only one provider exists, the trait's `locked` status is unused in the MVP and the env file's `provider:` key is kept so files stay forward-compatible.

### Keyring

Entries named `hopper/<workspaceId>/<environment>/<variable>`. The env file is committed with only `{ provider: keyring }`, so after cloning a repo each coworker sees a checklist ("3 secrets to fill in") and sets their own values through a password field. Known limit: Windows Credential Manager blobs max out at about 2.5 KB, which rules out PEM bundles; those wait for a DPAPI-encrypted file provider.

### Bitwarden: deferred

No Bitwarden integration in the MVP or the next phases. Users copy the key from Bitwarden into Hopper's password field once per environment; it goes to the OS credential store. The `SecretProvider` trait and the `provider:` key in env files exist so a `bw` provider can be added later without a format change (design notes for that day: unlock via `bw unlock --passwordenv`, session key in memory only with an idle timeout, short in-memory value cache; none of it is built now).

### What the UI shows

- Secrets render as `••••••••` with a status dot (set / missing). Plain variables show their value.
- In URL, header and body editors, `{{` autocompletes variable names (type shown, never secret values); unresolved variables are underlined red; a "resolved URL" line under the URL bar shows the final URL with secrets masked.
- Environment switcher in the panel header with kind color (production red tints the whole panel's top edge); per-user overrides from `*.local.yaml` are marked "local".

---

## 7. App structure

```
hopper/
  .node-version  Cargo.toml (workspace)  package.json  CLAUDE.md  README.md
  crates/hopper-core/src/
    format/    workspace, collection, folder, request, environment, migrate, yaml (canonical writer)
    vars/      resolver, builtins, secret-aware substitution
    secrets/   mod (trait), keyring, redact
    openapi/   parse (2.0/3.0/3.1), refs, example, index, diff
    search/    index, query parser, ranking
    engine/    client, build, send, response, curl
    policy/    agent access per request, environment rules, pattern rules, fingerprints, host binding, approvals
    store/     workspace service, watcher (notify), atomic write, revisions, trash
    history.rs
  src-tauri/src/
    main.rs lib.rs state.rs
    ipc/       workspace.rs request.rs env.rs secrets.rs spec.rs run.rs history.rs settings.rs
    mcp/       server.rs auth.rs tools.rs approval.rs
    panel.rs   (window builder, visibility state machine, place/drag/pointer/focus handoff; ported from Spawner's companion.rs)
    hotkey.rs  tray.rs  updater.rs
  src/
    ipc/       typed wrappers + generated types + dev/mockBackend.ts (browser dev, like Spawner)
    panel/     Panel, Mascot, place, view, control   (ported from src/companion; mascot art is original)
    store/  components/  styles/tokens.css
    views/     find (home), request, response, recent
    sheets/    collections, environments, secrets, agent (activity, approvals, grants, MCP setup), settings
  fixtures/    format/vN/   openapi/ (swashbuckle 3.0, swagger 2.0, 3.1, 3000-op synthetic)
  schemas/     generated JSON Schemas
  docs/        format.md  mcp.md  design/
  .github/workflows/  ci.yml  release.yml (from Spawner)
```

### State sharing

```
AppState { workspace, secrets, engine, policy, approvals, events }   // Arc'd services in hopper-core
   ipc::*  --call-->  services  <--call--  mcp::tools     (adapters stay thin; policy lives in services)
services --events--> Tauri events + MCP activity feed
```
Rust owns the truth. The frontend keeps a cache and sends patches. Open editors are drafts with explicit save (Ctrl+S) because the files are versioned; if the file changes on disk while a draft is dirty, a banner offers reload or keep mine.

### IPC contract (Tauri commands, TS types generated)

`workspace_open/recent/close`, `tree_get`; `request_get/save/delete/move/duplicate`; `env_list/get/save/set_active`; `secret_status/set/clear/reveal`; `pattern_rule_list/add/remove`; `spec_import/reimport_preview/reimport_apply`, `search_routes`; `request_send(runId)`, `request_cancel(runId)`; `history_list/get`; `mcp_status`, `agent_access_get/set` (per request, bulk), `agent_env_policy_get/set`, `approval_respond`; `settings_get/set`; `panel_place/drag/pointer/focus/set_width`, `hotkey_get/set`.
Events: `workspace://changed`, `run://progress`, `secrets://status`, `agent://activity`, `agent://approval-request`, `panel://summon`, `panel://show-request`, `update://available`.
Errors are a typed `{ code, message, hint? }`; the UI maps codes to in-app dialogs. The rule from Spawner holds: no native dialogs, only the folder picker; `dialog:allow-message` is never granted. This goes in `CLAUDE.md` on day one.

### Testing

- **hopper-core** (fast, no Tauri): format round-trip and golden-fixture migration tests; resolver (precedence, unresolved, secret tracking, scrubbing); OpenAPI fixtures including real Swashbuckle output, Swagger 2.0, 3.1, circular `$ref`, duplicate `operationId`; search ranking tests on a fixture with many `User*` controllers; re-import diff cases (changed, new, removed, moved, user-edited values preserved); engine against a local axum server (redirect chain, timeout, cancel mid-stream, gzip/brotli, multipart, cookies) and an `rcgen` self-signed/internal-CA server for TLS; secrets with an in-memory fake provider; policy matrix, per-request agent access (Blocked wins, local always allowed, fingerprint suspension after an MCP edit but not after a UI edit) and host-binding tests.
- **MCP**: integration tests that start the server on a random port and call it as a JSON-RPC client: every tool, token and Origin rejection, `expectedRevision` conflicts, secret values never appearing in any output (property test over all tools).
- **Panel**: the visibility/focus state machine (hidden, tab, compact, open; passive versus active show; focus handoff) is a pure function with unit tests, as Spawner tests its `Companion` rules; Win32 calls stay behind thin functions verified by hand in the spike.
- **Frontend**: Vitest for store and logic; Playwright against `pnpm dev` with the mock backend (the Playwright MCP is available here) at **430, 640 and 860 px viewports** for search, editor, response, and env flows. A `tauri-driver` smoke test is a phase 2 item.
- **CI** (`ci.yml`, new, Spawner has none): `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test -p hopper-core`, generated-types freshness, `pnpm typecheck`, `pnpm test`.

---

## 8. Design direction

The panel inherits Spawner's companion look (tokens, 4 px radius, hairline dividers, Inter and JetBrains Mono) but with its own neutrals and accent so two docked panels are never confused. The design problem is density: a request client in a 430 px column.

### Three directions

**A. Dock (recommended)**: a slim, tall panel on the screen edge. Single column; the search box on top is the home screen; picking a result opens the request with the response stacked below it (side by side at 640+). Mascot-driven states, compact pinned ticker. Dark first, light available.

**B. Spotlight**: a launcher-style box that appears near the top-center on the hotkey, results below, Enter expands it into a card with the response, and it vanishes afterwards. Fastest for a one-off call, but it has no persistent presence for agent activity or approvals and is awkward for long editing and big responses.

**C. Dock with voxel ornament**: A with heavier Minecraft flavor (chunky borders, inventory-slot grids, pixel icons). Strongest nod, but ornament costs scarce width and it sits closest to trademark territory.

### Recommendation: A, with B's summon behavior and a little of C

- The hotkey-then-type flow comes from B; the persistent edge presence, approvals and agent activity come from A.
- The Minecraft nod, no assets: the mascot is a funnel into a box (a generic object) drawn in the same pixel-stepped style as the wordmark (Silkscreen is already a Spawner font dependency); an 8 px grid; environment chips drawn like inventory slots. Mascot animation: a few square "items" drop through the funnel while a request is in flight, and the box fills when the response arrives (under 200 ms per beat, disabled with reduced motion).

### Panel anatomy (open state)

1. **Header**: drag handle and mascot, workspace name, **environment switcher** (kind-colored), pin, width toggle (430/640/860), menu (sheets).
2. **Find (home)**: search box with facet chips (method, tag), result list with highlighted path and tag chip, a Browse button (collections sheet), and the Recent strip below.
3. **Request view**: method + URL line, resolved-URL line, Send/Cancel, **agent access chip (Ask / Auto / Blocked)**, sections as collapsible blocks (Params, Headers, Body, Auth, Notes, Source) that remember what was open.
4. **Response**: status, time, size, TLS-off badge; Body (Pretty/Raw/Preview), Headers, Cookies, Timeline; history dropdown; Copy cURL and Copy body; "maximize" fills the panel for large JSON, and back.
5. **Sheets** (slide over the panel, never new windows): Collections (tree, orphan and agent-access markers), Environments and secrets (variables, missing-secrets checklist, Reveal/Clear), Agent (live activity, approvals, grants with revoke, per-environment policy, MCP setup guide), Settings (workspace, hotkey, dock side, width, autostart, theme, updates).
6. **Compact (pinned) ticker**: last request name, status chip, time, plus a line for agent activity or a pending approval.

### Mock up first (HTML, in a separate design thread, before UI code)

1. The open panel at 430 and 640 px: Find with a spec of dozens of `User*` controllers, then the request and response stacked.
2. Mascot states and the compact ticker: sleeping, sending, agent working, needs approval, error, tucked.
3. The approval prompt in the panel (resolved request, masked secrets, Allow once / Always allow / Block / Deny).
4. The environments and secrets sheet with missing secrets and a production environment active.
Then dark and light of the winner and the first-run state (pick a workspace, import a spec, connect an agent).

---

## 9. Phased roadmap

**Phase 0: foundation and spikes (about 1 week)**
Scaffold from Spawner conventions (Tauri, Vite, tokens, the panel window, `CLAUDE.md` rule, `ci.yml`, `release.yml` with a signing key and a test tag). Spikes with go/no-go: (a) reqwest 0.13 against an internal-CA host on Windows (fallback `native-tls`); (b) `rmcp` Streamable HTTP inside Tauri's runtime (fallback hand-rolled); (c) `keyring` behavior and size limit on Credential Manager; (d) YAML crate choice and canonical writer; (e) TS type generation; (f) **panel behaviors on Windows**: non-activating show, hotkey summon with focus handoff, typing and IME in the transparent webview, CodeMirror in it, width resize in place, multi-monitor docking (most is proven in Spawner; this confirms it with real text input). Output: format v1 types and schemas, secret and MCP contracts.

**Phase 1: MVP, useful on day one**
The panel itself (hotkey summon, docking, states, mascot, tray, autostart, width presets) and, inside it: open a workspace folder; import a spec by URL/file; **search palette**; open an operation as a draft and save it as a request; environments with variables and **keyring secrets**; engine core (all methods, JSON/text/form bodies, bearer/basic/API key, pretty JSON, headers, TTFB/total, size, cancel, copy cURL, TLS-skip toggle); **MCP with orient, find, write and `hopper_send_request`**, token and Origin hardening, per-request agent access (Ask/Auto/Blocked), local-always-allowed, in-app approval; setup guide; NSIS installer and updater. This replaces "ask the AI for a cURL": the AI writes the request into Hopper, and the user sends it with the key already resolved.

**Phase 2: team-ready**
spec re-import diff UI and `reimport` tool; history; Agent screen for grants and pattern rules, bulk grants by folder; cookies jar UI and redirect chain; multipart and binary bodies; folder/collection auth inheritance; Recent strip and pinned requests; paste-a-cURL import; `*.local.yaml` overrides; tray persistence; `hopper-mcp` stdio shim; Playwright e2e in CI.

**Phase 3: ecosystem**
Bruno (OpenCollection YAML or `.bru`) and Postman import/export; OAuth2 client credentials; response captures into session variables (light chaining, still no scripting engine); GraphQL; DNS/connect/TLS timing; code snippets; other secret providers (`env`, Bitwarden, 1Password); **Spawner link** (read a `local` base URL from Spawner's MCP or its port file); compare a response across environments; macOS/Linux builds.

**Not planned**: a main window or fullscreen mode, scripting runtime, cloud sync or accounts, mock servers, load testing.

Task routing inside phases: Sonnet writes the file format, secret provider, policy and MCP/IPC contracts and reviews risky diffs; Haiku does scaffolding, copying Spawner workflows and styles, fixtures, repetitive tool/IPC consumers, UI components against a fixed contract, docs, and tests for existing code. A check failing twice returns the task to Sonnet.

---

## 10. Risks and open questions

**Risks**
- **SmartScreen (accepted).** The installer is unsigned, so every first install warns; the README documents "More info > Run anyway", as Spawner's does. Revisit with Azure Trusted Signing or a company certificate, or a winget/Scoop manifest, if adoption suffers. The Tauri updater signature is separate and does not remove the warning.
- **Updater needs an anonymous `latest.json`.** Satisfied by the public repo; do not make it private without moving the artifacts.
- **Shared collections in git.** Merge conflicts are limited by one-file-per-item and alphabetical ordering, but spec links and base URLs differ per person (hence `*.local.yaml` overrides); keyring entries are per machine, so every coworker re-enters values once; AI churn must stay deterministic or diffs get noisy.
- **TLS and proxy on corporate Windows.** Platform verifier should honor the OS store, but it must be proven in the spike; PAC scripts and NTLM proxies are unsupported by reqwest.
- **Prompt injection through spec or response text** into an agent that can send authenticated requests: addressed by host binding, per-environment policy, user-local policy storage, and redaction, but it is the highest-consequence area, so it gets the heaviest tests.
- **Credential Manager blob limit** (about 2.5 KB) for large secrets.
- **OpenAPI in the wild**: huge documents, circular refs, `allOf`/`oneOf` example generation, specs behind auth, wrong `servers` entries.
- **YAML ecosystem**: `serde_yaml` is archived; comment loss on write is a known trade-off.
- **Cramped editing in a side panel.** Mitigated by width presets, progressive disclosure, a maximize-response action, and mock-ups at 430 px before any UI code; if it fails the real-world test, a pop-out is a phase 3 option, not a main window.
- **Always-on-top annoyance and focus bugs.** Hidden when idle, passive show never takes focus, approvals ignore early keystrokes; the global hotkey may conflict with other tools (configurable, tray as fallback). Corporate security tools sometimes flag global hotkeys and always-on-top transparent windows.
- **Win32-specific window code** (accepted: Windows first; non-Windows fallbacks exist as in Spawner).
- **Transparent webview + IME/text input** is the likeliest source of surprises, hence spike (f).
- **WebView2** on older Windows 10 (installer should bootstrap it).
- **Name**: Hopper is also an existing reverse-engineering tool and a travel app; harmless for an internal tool, worth a check before public distribution. No Minecraft assets anywhere.

**Resolved questions**: public repo `uFleck/hopper`; unsigned installer; flexible workspace placement; no Bitwarden for now; per-request agent access chosen by the user (Ask/Auto/Blocked), `local` always allowed.

**Still open (non-blocking)**: exact placement of the agent-access chip and bulk controls (settled in the mockups).

---

## Verification (for the first phase)

- `cargo test -p hopper-core` passes: format round-trip, frozen v1 golden fixtures, migration chain test harness (even with a single version), canonical writer idempotence.
- The spike report records pass/fail for each of (a) to (e) with the exact crate versions chosen.
- `pnpm install && pnpm tauri dev` starts with **no main window**: a tray icon appears, the hotkey shows the docked panel with the search box focused, Esc hides it and focus returns to the previous window, and a passive show does not steal focus; `pnpm dev` in a browser runs the panel against the mock backend.
- CI green on a PR; a throwaway tag builds a signed NSIS installer plus `latest.json` on a test release (deleted afterwards).
- A stub MCP endpoint on the probed port answers `initialize` and `tools/list` with the bearer token and rejects a request with a browser `Origin`.

Execution: Split (Sonnet does the hopper-core format v1 types and migration harness, the SecretProvider/agent-access policy/MCP tool contracts, the six spikes, and the panel window port with its visibility/focus state machine; Haiku does the Tauri + Vite scaffold, tokens and mascot placeholder, sheet and view shells at 430/640/860 against fixed contracts, ci.yml and release.yml adapted from Spawner, fixtures, and the browser mock backend)
