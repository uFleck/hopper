# Stage-based environments (handoff)

Status: decided, not yet planned or implemented. A planning agent hit the session rate limit (resets 4:10pm America/Sao_Paulo) before writing a plan. Resume from here in a fresh session.

## Problem

- An environment is one file `environments/<stem>.env.yaml` (plus `<stem>.local.yaml` for per-user overrides): `name`, `envKind` (`EnvKind` in `crates/hopper-core/src/format/types.rs:256`: Local, Dev, Staging, Production, Other), `variables`, `secrets` (name -> provider), `tls`, `allowedHosts`.
- The team has applications (collections: `bill`, `account`) and four stages (local, dev, rc, production). One env today mixes both: `bill-local`.
- New envs created through MCP are forced to `envKind: other` (`service/agent.rs`, the "new environments are created with kind `other`" guard), so `bill-local` is not `local`. `kind` duplicates the name.
- Variables exist only at environment level. Request inputs (`customerIdentifier`, `contractIdentifier`) got stuck there. Collections, folders and requests have no variables.
- Agents cannot set any variable whose name ends in `url` (`is_url_variable`, `service/agent.rs`), so `baseUrl` could not be defined and `{{baseUrl}}` cannot resolve.

## Decisions (user confirmed)

1. Stages are fixed: `local`, `dev`, `rc`, `production`. No user-created stages. `envKind` is removed; the stage name is the identity. Trust rules keyed on EnvKind (local always allowed for agents, approvals elsewhere) re-key to the stage name `local`.
2. Environments hold one section per application (application = collection slug). A section holds `baseUrl` and other URL variables, plus its secret declarations. Shape: `environments/<stage>.env.yaml` with `applications: { <slug>: { variables, secrets, ... } }`. Open: whether tls/allowedHosts live per application section (recommended) or per stage.
3. Variables also at workspace, collection, folder and request level. Precedence, highest first: request > nearest folder > collection > stage+application section > workspace defaults.
4. Agents CAN set `baseUrl` and other variables for a stage+application section. My guard, to be confirmed: sends to `local` never prompt, so in `local` an agent-set URL variable must resolve to loopback (`localhost`, `127.0.0.1`, `::1`); otherwise refused. Without it an agent could point `local` at a remote host and send secrets with no prompt.
5. Secrets keyed per (stage, application, name). Agents may declare secrets per stage+application, never read or set values.
6. Migration of the user's workspace `C:\umbler`: `environments/bill-local.env.yaml` becomes stage `local`, application `bill`. Its secret `billApiKey` (keyring, status set) moves to the new keyring key. Never lose a value; if a move fails, keep the old entry and report it.

## Code facts gathered

- Keyring secret reference: `SecretRef { workspace_id, environment, variable }` (`crates/hopper-core/src/secrets/mod.rs`).
- Request binding for grants: `Binder::binding` (`service/mod.rs`) calls `send::binding_hash(host, allowed, collection, folders, env_stem)` (`service/send.rs:262`). It includes collection and folder headers/auth, so editing them suspends Auto grants. Keep that behavior.
- Variable layers: `prepare::Layers` has workspace_id, defaults, collection, folders, environment, local_overrides, overrides, secrets, agent, file_root (`crates/hopper-core/src/prepare/`).
- Format: version 1, fixtures in `fixtures/format/v1`, JSON Schemas generated from `crates/hopper-core/src/schema.rs` into `schemas/`, migrations in `crates/hopper-core/src/format/migrate.rs`.
- Agent env tools today: `hopper_upsert_environment`, `hopper_list_environments`, `hopper_get_environment`, `hopper_delete_environment`, `hopper_set_active_environment` (`src-tauri/src/mcp/tools.rs`).
- UI env sheet and IPC: `src-tauri/src/ipc/env.rs`, `ipc/secrets.rs`; frontend under `src/`.
- Activity log is in memory only (`log_activity`, `service/mod.rs:524`; `open.activity`). Persisting it is a separate item.

## Already done this session

- Get-started bug fixed in `src/views/FindView.tsx`: the setup card no longer replaces the saved-request list when the workspace has no spec. `pnpm typecheck` passes; not yet run in the app.
- The running Hopper build predates the MCP collection/folder/environment tools added earlier. Rebuild before the agent can use them.

## Next steps

1. Plan: file-level design (format, store, prepare, secrets, service, MCP, UI, migration, tests), then waves of Haiku implementers on disjoint files, with narrow checks (`cargo test -p hopper-core <filter>`).
2. Confirm the loopback guard (decision 4) with the user before implementing it.

---

# Plan (written by the planner; verified against the code on 2026-10-27)

Decisions I made where the handoff left them open (the user may overrule):
- D-a. `tls` and `allowedHosts` live per application section (handoff recommendation).
- D-b. Bump the format to **2** (additive fields on other kinds, a breaking shape for `environment`). Old Hopper opens v2 files read-only; fine.
- D-c. Stage = file stem. `environments/<stage>.env.yaml`, stage in `local|dev|rc|production`. The `name` key and `envKind` are gone. Any other stem is a LoadProblem, never silently used.
- D-d. A declared secret name wins over every variable layer except per-send `overrides` (a request/folder variable can never shadow or leak a secret).
- D-e. `<stage>.local.yaml` overrides sit directly above the stage+application section and below collection (chain below).
- D-f. v1 environment files are NOT auto-converted. They load as "legacy" and the user runs an explicit migration (writes the stage file, moves keyring entries). Nothing touches the keyring on open.
- D-g (REVISED after user approval, supersedes the earlier text). Non-`local` stages: an agent may set any URL variable to any host. Sends still need the user's approval (existing policy). To keep secret destinations under user control: when an agent changes a URL variable in a section that has no explicit `allowedHosts`, the write first pins `allowedHosts` to the section's current derived list, so the new host is not allowed for secrets until the user adds it (UI or `allowedHosts`). Derived allowed hosts come only from the stage+application section variables, never from collection/folder/request variables.
- D-h. `hopper_delete_environment` is removed for agents (stages are fixed). The UI keeps "remove application section".

## 1. Data model and file format

### 1.1 Types (`crates/hopper-core/src/format/types.rs`)
- New `Stage` enum (`Local, Dev, Rc, Production`, serde lowercase) with `ALL`, `as_str()`, `FromStr`, `is_local()`. Exported from `format`.
- New `AppSection` (camelCase): `variables: BTreeMap<String,String>`, `secrets: BTreeMap<String,SecretDecl>`, `tls: Tls`, `allowed_hosts: Option<Vec<String>>`; all skip-if-default; `Default` derive (an absent section behaves as empty).
- New v2 document `StageEnvironment { hopper, kind, applications: BTreeMap<String, AppSection>, legacy: Option<LegacyEnvironment>, extra }`; `document!(StageEnvironment, Environment, "environment")`. Application keys are collection slugs (no `/`, `\`).
- `LegacyEnvironment { name, env_kind: EnvKind, variables, secrets, tls, allowed_hosts, extra }`: the old v1 body. `EnvKind` stays, documented legacy-only (used only to suggest a stage: local->local, dev->dev, staging->rc, production->production, other->none).
- `variables: BTreeMap<String,String>` (`#[serde(default, skip_serializing_if = "BTreeMap::is_empty")]`) added to `WorkspaceDefaults`, `Collection`, `Folder`, `Request`, placed after `headers`/`auth` and before `extra` (key order = declaration order). Workspace-level key is `defaults.variables` in `hopper.yaml`.
- Keep old `Environment` and `EnvKind` users compiling until wave 4 (bridge), then delete the old `Environment` struct.

### 1.2 Stage file (canonical)
```yaml
hopper: 2
kind: environment
applications:
  bill:
    variables:
      baseUrl: http://localhost:5080
      userLogin: bruno.koster@umbler.com
    secrets:
      billApiKey:
        provider: keyring
    tls:
      insecureSkipVerify: true
    allowedHosts: [localhost]
```
Local overrides: `environments/<stage>.local.yaml` (gitignored by the existing `*.local.yaml` rule): `applications: { bill: { variables: { baseUrl: ... } } }`. Only `applications.*.variables` is read. A top-level `variables:` key in a `<stage>.local.yaml` is a v1 leftover: LoadProblem "legacy local overrides, run the environment migration".

### 1.3 Version and migration (`format/mod.rs`, `format/migrate.rs`)
- `CURRENT_VERSION = 2`; `STEPS = [v1_to_v2]` (the const assert on length already guards this).
- `v1_to_v2(Value)`: if `kind == environment`, move every key except `hopper`/`kind` under a `legacy:` mapping, set `applications: {}`. For every other kind it is the identity (the new `variables` fields are optional). The step never needs the file stem, which is why stage/app mapping is done by the explicit migration in 6, not here.
- Files are never rewritten by loading (existing rule); v2 written only on save.
- Update the doc comment in `migrate.rs` and the `format/mod.rs` header ("Format v1" -> "v2").

### 1.4 Schemas and fixtures
- `schema.rs`: `("environment", schema_for!(StageEnvironment))`.
- Freeze nothing new under `fixtures/format/v1` (they stay as the v1 input corpus; `environment.expected.yaml` is regenerated and will show the `legacy:` shape).
- Add `fixtures/format/v2/{workspace,collection,folder,request,environment}.yaml` inputs (collection/folder/request/workspace carry `variables`; environment has two applications) and generate `*.expected.yaml`.
- Regenerate with `UPDATE_SCHEMAS=1 cargo test -p hopper-core schema` and `UPDATE_FIXTURES=1 cargo test -p hopper-core format::`; review the diff, then rerun without the env vars.
- `examples/workspace/environments/{local,public}.env.yaml` become v2 stage files (`public` -> `production`, application = the example collection slug); regenerate with `UPDATE_EXAMPLES=1 cargo test -p hopper-core --test example_workspace`, then review.

## 2. Store and prepare

### 2.1 Store (`store/mod.rs`, `store/load.rs`, `store/watch.rs`)
- `EnvNode` becomes `{ stage: Stage, env: Loaded<StageEnvironment>, local_overrides: BTreeMap<String /*application*/, BTreeMap<String,String>> }`. `Tree.environments` keeps the name, holds only valid v2 stage files. Add `Tree.legacy_environments: Vec<LegacyEnvNode { stem, legacy: LegacyEnvironment, revision, local_overrides: BTreeMap<String,String> }>` for v1 files (doc.legacy is Some).
- `load.rs::environments()`: stem must parse as a `Stage`, else `problem("environments/<file>", "not a stage: expected local, dev, rc or production")` and skip. A parsed doc with `legacy` goes to `legacy_environments`. `local_overrides()` reads `applications.<app>.variables` (scalars stringified as today).
- `watch.rs` already watches `environments/`; confirm `.local.yaml` and `.v1.bak` events do not break the debounce (read, do not change unless a test fails).
- Replace `upsert_environment(name, edit, rev)` by `upsert_stage(stage, edit: FnOnce(&mut StageEnvironment), rev)` (same revision/read-only/atomic-write logic, new file = empty `applications`) and `delete_environment` by `remove_application(stage, app)` (edit, keeps the file). Replace `environment_on_disk(stem)` by `stage_on_disk(stage)`. Drop `env_stem`.
- New `upsert_variables(target: VarTarget, edit: FnOnce(&mut BTreeMap<String,String>), expected_revision)` with `VarTarget { Workspace, Collection(slug), Folder{collection, path}, Request(id) }`, built on existing `update_collection`/`update_request` plus a new `update_folder(collection, path, edit, rev)` and a workspace-defaults writer. Returns the new revision.

### 2.2 Prepare (`prepare/mod.rs`)
`Layers` loses `environment: &Environment` and gains:
```rust
pub stage: &'a str,              // "" = no environment selected
pub application: &'a str,        // collection slug
pub section: &'a AppSection,     // empty when the stage has no such application
pub request_vars: &'a BTreeMap<String,String>, // request.variables (mask_template has no Request)
// local_overrides: &BTreeMap<String,String> now already narrowed to this application
```
`Resolver::lookup` order, first hit wins:
1. per-send `overrides`; 2. `$builtin`; 3. `section.secrets` (secret; D-d); 4. `request_vars`; 5. `folders` nearest first (`layers.folders` is outermost first, iterate reversed); 6. `collection.variables`; 7. `local_overrides`; 8. `section.variables`; 9. `defaults.variables`; else `Value::Missing`.
- Precedence mapped: request > nearest folder > collection > (local overrides) > stage+application > workspace defaults.
- Unresolved behavior is unchanged: `PrepareError::Unresolved { names }` before any secret fetch; single-pass substitution (a substituted value is never rescanned).
- `SecretRef` built with `SecretRef::new(workspace_id, stage, application, name)`. `scope` stays `"{ws}/{stage}"`. `insecure_tls` and `allowed_hosts(...)` read `section`. `allowed_hosts(section: &AppSection)` keeps its semantics (explicit list, else hosts of section variables whose name ends in `url`).
- Only two non-test `Layers { .. }` literals exist: `service/mod.rs:225` and `service/send.rs:380`.

## 3. Policy and secrets

### 3.1 Policy
- `policy/mod.rs`: `SendContext.env_kind: EnvKind` -> `stage: Stage`; line 129 becomes `ctx.stage == Stage::Local`. `service/mod.rs::rules_for(data, stage, ...)` default `safe_reads: matches!(stage, Stage::Dev | Stage::Rc)`; stored rules stay keyed by the stage name string (`local`, `dev`, ...). `send.rs:596` check becomes `Stage::Production`. `ApprovalRequest`/`AskInfo` carry `stage` + `application` instead of `env`/`env_kind`.
- Grants: `binding_hash(..., env_stem)` keeps its signature, passing the stage name. After migration, Auto grants bound to `bill-local` ask once more (binding changes); accepted and noted in the report.

### 3.2 Loopback guard (decision 4, needs user confirmation before wave 3)
In `service/agent.rs`, new `fn agent_url_guard(stage, section_explicit_hosts, name, value) -> Result<(), HopperError>` applied to every agent-set variable in a stage section whose name ends in `url` (case-insensitive):
- `Stage::Local`: value must parse as an http(s) URL whose host is `localhost`, `127.0.0.1` or `::1` (literal, no `{{` in the host). Otherwise `denied`: "in `local` agent-set URLs must be loopback".
- other stages (REVISED, D-g): any host is accepted. If the section has no explicit `allowedHosts`, pin them to the current derived list in the same write (see D-g), so the new host gets no secret access until the user adds it.
The old `is_url_variable`/`protected` refusal in `upsert_environment` is deleted. Removing a URL variable (`removeVariables`) is allowed in `local`, denied elsewhere (it can retarget requests).

### 3.3 Secret key scheme
- `SecretRef { workspace_id, environment, variable }` keeps its three fields; `environment` now holds `"<stage>/<application>"`. Add `SecretRef::new(ws, stage, application, variable)`; `key()` is unchanged and yields `hopper/<ws>/<stage>/<application>/<variable>`. The legacy ref is `SecretRef { workspace_id, environment: "<old stem>", variable }` (`hopper/<ws>/bill-local/billApiKey`). No change to `keyring.rs`/`SecretProvider`. Stage and application are validated not to contain `/`.
- Agents may declare secrets per stage+application, never set or read values (unchanged).

### 3.4 Moving a secret without losing it (`service/legacy_env.rs`, new)
Per declared legacy secret: `get(old)`; if NotFound -> record "was not set", nothing to move. Else `set(new)`, `get(new)` and compare; only then `clear(old)`. Any error: leave the old entry, clear nothing, add `{name, reason}` to `failed`. Re-running is safe (idempotent: if `new` already equals `old`, just clear old). The old file is renamed to `<stem>.env.yaml.v1.bak` only when `failed` is empty; otherwise it stays and the stage file is still written (variables are not lost either way).

## 4. Service, MCP and UI

### 4.1 DTOs (`service/dto.rs`)
- Remove `EnvironmentDto`, `EnvironmentInput`, `env_kind` everywhere.
- Add `StageDto { stage: Stage, active, applications: Vec<ApplicationEnvDto>, revision: Option<String>, read_only, agent_rules }` (always four, an absent file yields an empty one).
- `ApplicationEnvDto { application, variables: Vec<EnvVariable>, secrets: Vec<EnvSecretDto>, tls, allowed_hosts, allowed_hosts_explicit }`.
- `ApplicationEnvInput { stage, application, variables: Vec<NameValue>, secrets: Vec<NameProvider>, tls, allowed_hosts?, expected_revision? }`.
- `LegacyEnvDto { stem, suggested_stage?, suggested_application?, secret_names, variable_names }`, `MigrationReport { stage, application, moved: Vec<String>, failed: Vec<{name, reason}>, not_set: Vec<String>, old_file_kept: bool }`.
- `VarTargetDto` (`workspace | collection{slug} | folder{collection,path} | request{id}`), `VariablesDto { target, variables: Vec<NameValue>, revision }`, `VariablesInput`.
- `ApprovalRequest`: `env`/`env_kind` -> `stage`, `application`. Event `SecretsStatus { stage, application }`. `SendRequest.env` is the stage.

### 4.2 Service functions (`service/mod.rs`, `send.rs`)
- `environment_list() -> Vec<StageDto>`; `environment_save(ApplicationEnvInput) -> StageDto` (edits one section; keeps the "user-local override must not leak into the shared file" logic per application); `environment_delete(stage, application)`; `environment_set_active(Option<Stage>)` (state still stores a string, `"local"`; a stale non-stage value loads as none).
- `secret_ref(stage, application, name)` replaces `secret_ref(env, name)` and checks the declaration in that section; `secret_set/clear/reveal(stage, application, name, ...)`.
- `environment_legacy_list()`, `environment_migrate_legacy(stem, stage, application) -> MigrationReport` (in `service/legacy_env.rs`; it also moves `<stem>.local.yaml` variables into `<stage>.local.yaml`, copies stored agent rules `<stem>` -> `<stage>` only if the stage has none, and rewrites a stored `active_env == stem` to the stage). New file declared by `mod legacy_env;` in `service/mod.rs` (owned by the service task).
- `variables_get(target)`, `variables_save(VariablesInput)` over `Store::upsert_variables`.
- `send.rs`: `EnvSnap { stage: Option<Stage>, section: AppSection, local: BTreeMap }`; `env_snapshot(s, stage: Option<Stage>, application: &str)`; the application is `snap.collection` slug (add `collection_slug` to `Snapshot`). `env_valid`, `env_stem_for`, `check_env_name`, `empty_environment` are deleted.

### 4.3 MCP (`src-tauri/src/mcp/tools.rs`, `service/agent.rs`)
- `hopper_upsert_environment` args: `{stage, application, variables, removeVariables, secrets, expectedRevision}`; `tls`/`allowedHosts` still accepted only to be refused; `envKind` removed. Uses 3.2.
- `hopper_list_environments`, `hopper_get_environment {stage}` return four stages with per-application non-secret variables, `localVariableNames`, secret `{provider,status}`, allowed hosts, revision.
- `hopper_set_active_environment {stage|null}`; `hopper_send_request.environment` = stage; `hopper_workspace` lists stages and `activeEnvironment`.
- Removed: `hopper_delete_environment` (and the `delete_environment` fn in agent.rs).
- Added: `hopper_set_variables {scope: "workspace"|"collection"|"folder"|"request", collection?, folder?: [slug], requestId?, variables, removeVariables, expectedRevision}`; no URL guard at these layers (they cannot widen allowed hosts, and a changed host changes the binding hash).
- `INSTRUCTIONS` (tools.rs lines 19-20) changes to: "...requests to the `local` stage never ask. Stages are fixed: local, dev, rc, production. An environment is a stage with one section per application (collection slug). You can set a section's variables and declare secret references; you cannot set secret values, TLS or allowed hosts. In `local`, URL variables must point at localhost, 127.0.0.1 or ::1; in other stages the host must already be in the user's allowed hosts. Collection, folder and request variables go through hopper_set_variables. Precedence: request > folder > collection > stage+application > workspace."
- `hopper_describe_format` text and `service/agent.rs` hints that mention `envKind` are updated.

### 4.4 Tauri adapters and UI (thin, no policy)
- `src-tauri/src/ipc/env.rs`: `environment_list`, `environment_save {env: ApplicationEnvInput}`, `environment_delete {stage, application}`, `environment_set_active {stage}`, new `environment_legacy_list`, `environment_migrate_legacy {stem, stage, application}`, `variables_get`, `variables_save`. `ipc/secrets.rs`: `secret_set/clear/reveal {stage, application, name, ...}`. Register new commands in `src-tauri/src/lib.rs`.
- `src/ipc/types.ts`: drop `EnvKind` (except legacy), `EnvironmentDto`, `EnvironmentInput`; add the DTOs above; `ApprovalRequest` gets `stage`/`application`; events `secrets://status {stage, application}`; update the `Commands` map (lines ~430-460).
- `src/store/*`: `environments` holds `StageDto[]`; `selectActiveEnv` returns the active stage; `setActiveEnv(stage)`.
- `src/views/EnvLever.tsx`: four fixed notches (`local|dev|rc|production`), production guard on `stage === "production"`; `envClass` keyed by stage (`src/ui`).
- `src/sheets/EnvironmentsSheet.tsx`, `sheets/env/{EnvEditor,draft,SecretsList,AgentRules,api}.ts*`: pick stage (fixed list, no create/delete stage), then application (select from collections plus "add application"), edit that section. No `envKind` selector. A "Legacy environments" banner lists `environment_legacy_list` with prefilled stage/application and a Migrate button showing the `MigrationReport` (failed secrets listed by name).
- Collection/folder/request variable editing in the UI is out of scope for this change (agents and files cover it); the UI only reads them for resolution. Mark as a follow-up.
- `src/sheets/agent/ApprovalCard.tsx`, `src/views/request/*`, `src/ipc/dev/mockBackend.ts`, `src/ipc/dev/mock/{seed,send}.ts`, `src/ipc/contract.test.ts`, `sheets.test.ts`, `store.test.ts`: follow the type changes.

## 5. Tests (hopper-core, `cargo test -p hopper-core`)
Change: `service/tests.rs` harness `h.env(name, kind, secrets, tls)` becomes `h.env(stage_str, secrets)` writing application `app` of a stage file for the harness collection; ~70 call sites (`EnvKind::Local`->`"local"`, `Dev`->`"dev"`, `Staging`->`"rc"`, `Production`->`"production"`, `Other`->`"dev"` plus explicit stored `EnvRules { safe_reads: false, .. }` through the harness so prompting behavior is unchanged). `policy/tests.rs`, `prepare/tests.rs` (`Layers` fixture), `store/tests.rs`, `userdata/tests.rs` (only if it names `EnvKind`), `mcp/tests.rs` helper at line ~94 and the `envKind` assertions (line ~284).
Add (names):
- format: `v1_environment_migrates_into_legacy_block`, `v1_collection_loads_with_empty_variables`, `v2_variables_round_trip_canonical`, `stage_environment_unknown_keys_survive`.
- store: `loads_stage_files_and_ignores_unknown_stems_with_problem`, `local_overrides_are_read_per_application`, `legacy_v1_environment_is_listed_not_loaded`, `upsert_stage_creates_and_checks_revision`, `remove_application_keeps_other_sections`, `upsert_variables_on_collection_folder_request_and_workspace`.
- prepare: `precedence_request_over_folder_over_collection_over_stage_over_defaults`, `nearest_folder_wins`, `local_override_beats_stage_but_not_collection`, `secret_declaration_beats_request_variable`, `unresolved_variable_error_unchanged`, `secret_ref_uses_stage_and_application`.
- policy: `local_stage_allows_agent_without_prompt`, `safe_reads_default_for_dev_and_rc_only`, `host_binding_applies_in_local`.
- secrets: `key_scheme_is_stage_application_variable`, `legacy_and_new_keys_do_not_collide`.
- service/agent: `agent_can_set_base_url_in_local_loopback`, `agent_refused_remote_url_in_local`, `agent_refused_url_in_dev_without_allowed_host`, `agent_cannot_change_tls_or_allowed_hosts`, `agent_set_variables_scopes`, `agent_cannot_set_variable_that_is_declared_secret`, `send_resolves_collection_and_request_variables`, `environment_list_returns_four_stages`.
- legacy migration: `migrate_legacy_moves_secret_and_keeps_value`, `migrate_legacy_keeps_old_entry_when_set_fails`, `migrate_legacy_is_idempotent`, `migrate_legacy_moves_local_overrides_rules_and_active`, `migrate_legacy_not_set_secret_is_reported`.

## 6. Migrating the user's workspace `C:\umbler`
Workspace id `6b979f16-0814-4bd4-894c-6ceaad164d09` (`hopper.yaml`), collection `bill`, one env file `environments/bill-local.env.yaml` (v1: `userLogin`, `contractIdentifier`, `customerIdentifier`, `orgName`, secret `billApiKey`; no `baseUrl`, no local file).
1. Rebuild/restart Hopper after wave 6 (the running build predates these tools).
2. In the Environments sheet the legacy banner proposes stage `local`, application `bill` (stem `bill-local` = `<collection>-<stage>`). Click Migrate.
3. Result: `environments/local.env.yaml` (v2) with `applications.bill` holding the four variables and the `billApiKey` declaration; keyring entry `hopper/6b979f16-.../bill-local/billApiKey` copied to `hopper/6b979f16-.../local/bill/billApiKey`, verified, then the old entry cleared; old file renamed `bill-local.env.yaml.v1.bak`; active environment becomes `local`. If the copy fails the old entry and file stay and the report names `billApiKey`.
4. Follow-ups after migration (data, not code): move the three request inputs (`contractIdentifier`, `customerIdentifier`, `orgName`) from the section to `collections/bill/collection.yaml` `variables` (agent: `hopper_set_variables`), and set `applications.bill.variables.baseUrl` (the value is not in any file; ask the user. An agent may write it in `local` only if loopback). Check `{{baseUrl}}` resolves with `hopper_get_environment local`.
5. Auto grants for `bill` requests ask once more (binding hash changed); expected.

## 7. Execution

Gating strategy: while a wave deliberately breaks the crate (wave 3), per-task checks cannot compile; the orchestrator runs `cargo check -p hopper-core --lib 2>&1 | tail -40` at the wave end. After two bounced rounds Sonnet takes over. Rustc stops at unresolved imports, so errors are fixed in order. Never run the whole workspace; `cargo test -p hopper-core <filter>`, `pnpm typecheck`. Tauri compile only once at the end: `cargo check -p hopper`. Before wave 3, ask the user to confirm 3.2 (loopback guard and D-g).

Execution: Split (Sonnet does format, store, prepare, policy, service, agent guard, secret migration and the new tests; Haiku does SecretRef::new, fixtures and examples, Tauri adapters, UI, mechanical test conversion and docs)

Per-task budget: Haiku 15 tool calls, Sonnet 25. Each brief ends "do not commit, push or build; stop and report at the cap". Max 3 tasks per wave, one wave at a time.

## Status (orchestrator, after wave 5)
- Waves 1-5 done except: UI environment sheet (83 TS errors across 16 files), 13 hopper-core service test failures, MCP `INSTRUCTIONS` lost the "Hopper must be running" prefix (wave 4B) and `agent_send_outside_local...` fails, `tests/example_workspace.rs` imports removed `Environment`, fixtures/schema/examples still v1.
- `cargo test -p hopper-core --lib`: 285 passed, 17 failed (fixtures 1, schema 1, examples 1, service 13... see list in the run).
- Next: wave 6 = 6A (Sonnet, service test failures + MCP instructions), 6B (Haiku, fixtures/schema/examples/integration test), 6C (Sonnet, UI environment sheet and TS errors).

### Wave 1 (additive; crate stays green: `cargo check -p hopper-core --lib`)
- 1A Sonnet. Files: `format/types.rs`, `format/mod.rs`, plus one-line `variables: Default::default()` in every non-test struct literal of `Collection`, `Folder`, `Request`, `WorkspaceDefaults` (find with grep; files: `store/mod.rs`, `service/send.rs`, `service/mod.rs`, `service/agent.rs`). Change: 1.1 (Stage, AppSection, StageEnvironment, LegacyEnvironment, `variables` fields); do not bump the version, do not touch old `Environment`.
- 1B Haiku. Files: `secrets/mod.rs`, `secrets/keyring.rs` tests. Change: add `SecretRef::new(ws, stage, application, variable)` and the two tests in 5 (`key_scheme_is_stage_application_variable`, `legacy_and_new_keys_do_not_collide`). Check: `cargo test -p hopper-core secrets::`.
(Check after 1A+1B: `cargo check -p hopper-core --lib`.)

### Wave 2 (additive store; crate stays green)
- 2A Sonnet. Files: `store/mod.rs`, `store/load.rs`, `store/watch.rs` (read-only unless needed). Change: 2.1 added alongside the old APIs: new `StageNode` (the 2.1 `EnvNode` shape) in `Tree.stage_nodes`, `Tree.legacy_environments`, `upsert_stage`, `remove_application`, `stage_on_disk`, `upsert_variables`, `update_folder`; keep old APIs until wave 4. Check: `cargo check -p hopper-core --lib`.
- 2B Haiku. Files (new only): `fixtures/format/v2/*.yaml` (5 inputs as described in 1.4), `examples/workspace/environments/local.env.yaml` and `public.env.yaml` rewritten as v2 (use `ls examples/workspace/collections` for the application slug). No expected files, no cargo. Check: `git diff --stat` lists only those paths.

### Wave 3 (atomic cut-over; gate by orchestrator `cargo check -p hopper-core --lib`)
- 3A Sonnet. Files: `service/mod.rs`, `service/send.rs`, `service/dto.rs`, `service/approvals.rs`. Change: 3.1, 4.1, 4.2 (everything except agent.rs and legacy_env.rs); add `mod legacy_env;` and call-through stubs `environment_legacy_list`/`environment_migrate_legacy`.
- 3B Sonnet. Files: `service/agent.rs`. Change: 4.3 service side and the 3.2 guard, `set_variables`, `env_views` per stage, overview; delete `delete_environment`, `is_url_variable` use.
- 3C Sonnet. Files: `prepare/mod.rs`, `prepare/text.rs`, `policy/mod.rs`, new `service/legacy_env.rs`. Change: 2.2, 3.1 (policy), 3.4 (the migration function body, signature `pub(crate) async fn migrate(open: &Arc<Open>, stem, stage, application) -> Result<MigrationReport, HopperError>`).
Contract between 3A/3B/3C is the signatures in 2.2, 4.1, 4.2 above; no task edits another's file.

### Wave 4 (cleanup + adapters + UI data layer)
- 4A Sonnet. Files: `format/types.rs`, `format/mod.rs`, `format/migrate.rs`, `schema.rs`, `store/mod.rs`, `store/load.rs`. Change: delete old `Environment`, old store APIs and `Tree.environments` old meaning (rename `stage_nodes` -> `environments`); `CURRENT_VERSION = 2`, `v1_to_v2` (1.3); schema line. Check: `cargo check -p hopper-core --lib` green; grep `EnvKind` only in legacy code.
- 4B Haiku. Files: `src-tauri/src/ipc/env.rs`, `src-tauri/src/ipc/secrets.rs`, `src-tauri/src/lib.rs`, `src-tauri/src/mcp/tools.rs`. Change: 4.3 tool list/schemas/INSTRUCTIONS/dispatch and 4.4 adapters (thin wrappers only). Check: `cargo check -p hopper` (once).
- 4C Haiku. Files: `src/ipc/types.ts`, `src/store/state.ts`, `src/store/actions.ts`, `src/ui/*` (`envClass`), `src/ipc/dev/mockBackend.ts`, `src/ipc/dev/mock/seed.ts`, `src/ipc/dev/mock/send.ts`. Change: 4.4 types, store and mock to the DTOs. Check: `pnpm typecheck` (UI components fail until wave 5C; acceptable, only the listed files must be clean).

### Wave 5 (tests and UI components)
- 5A Sonnet. Files: `format/tests.rs`, `store/tests.rs`, `prepare/tests.rs`, `policy/tests.rs`, `userdata/tests.rs`. Change: convert and add the named tests for format, store, prepare, policy (section 5). Check: `cargo test -p hopper-core format:: store:: prepare:: policy:: userdata::`.
- 5B Haiku. Files: `service/tests.rs` (harness and ~70 call sites only; no new tests). Change: section 5 mapping. Check: `cargo test -p hopper-core service::` compiles; failing tests are reported by name, not rewritten.
- 5C Haiku. Files: `src/views/EnvLever.tsx`, `src/sheets/EnvironmentsSheet.tsx`, `src/sheets/env/*`, `src/sheets/agent/ApprovalCard.tsx`, `src/views/request/*`, `src/views/RequestView.tsx`, `src/App.tsx`, `src/ipc/contract.test.ts`, `src/sheets/sheets.test.ts`, `src/store/store.test.ts`, `src-tauri/src/mcp/tests.rs`. Change: 4.4 components, legacy banner, test fixtures. Check: `pnpm typecheck && pnpm test`.

### Wave 6 (new service tests, regeneration, final gate)
- 6A Sonnet. Files: new `service/tests_stages.rs` (+ `#[cfg(test)] mod tests_stages;` in `service/mod.rs`). Change: the service/agent and legacy-migration tests named in section 5. Check: `cargo test -p hopper-core service::tests_stages`.
- 6B Haiku. Files: `schemas/*.schema.json`, `fixtures/format/v*/*.expected.yaml`, `examples/workspace/**`, `README.md`, `docs/**` mentions of `envKind`. Change: run `UPDATE_SCHEMAS=1 cargo test -p hopper-core schema`, `UPDATE_FIXTURES=1 cargo test -p hopper-core format::`, `UPDATE_EXAMPLES=1 cargo test -p hopper-core --test example_workspace`, then the same three without the env vars; fix docs. Check: `cargo test -p hopper-core` green.
Final gate (orchestrator): `cargo test -p hopper-core`, `pnpm typecheck && pnpm test`, `cargo check -p hopper`, one Sonnet review pass over `git diff` for secrets/policy changes, then do section 6 with the user.
