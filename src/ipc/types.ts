// The IPC contract between the panel (React) and the Rust services. Hand-maintained, camelCase on the wire.
// The Rust side lives in `crates/hopper-core/src/service/dto.rs`; a Rust test writes sample JSON for every
// type into `fixtures/ipc/` and `src/ipc/contract.test.ts` checks it against these types, so drift fails CI.
// Secret values never appear here: the UI only sees `SecretStatus`. The one exception is `secret_reveal`.

export type Method = "GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "HEAD" | "OPTIONS";
export type AgentAccess = "ask" | "auto" | "blocked";
/** The four fixed stages. Serialized lowercase by the Rust `Stage` enum. */
export type Stage = "local" | "dev" | "rc" | "production";

/** Typed error every command rejects with. The UI maps `code` to an in-app dialog. */
export interface HopperError {
  code: string; // e.g. "revision_conflict", "unresolved_variables", "secret_missing", "not_approved", "read_only", "invalid_input", "denied" (details.reason), "confirm_unlisted_host" (details.host), "rate_limited", "timeout", "tls"
  message: string;
  hint?: string;
  /** Extra machine-readable context, e.g. `{ names: ["baseUrl"] }` for unresolved variables. */
  details?: Record<string, unknown>;
}

// ---------------------------------------------------------------- documents (mirror the file format)

export interface KeyValue {
  name: string;
  value: string;
  enabled: boolean;
}

export type Auth =
  | { type: "inherit" }
  | { type: "none" }
  | { type: "bearer"; token: string }
  | { type: "basic"; username: string; password: string }
  | { type: "apiKey"; name: string; value: string; in: "header" | "query" };

export type MultipartPart = { name: string; enabled: boolean } & ({ value: string } | { file: string });

export type Body =
  | { type: "none" }
  | { type: "json"; content: string }
  | { type: "text"; content: string }
  | { type: "xml"; content: string }
  | { type: "form"; fields: KeyValue[] }
  | { type: "multipart"; parts: MultipartPart[] }
  | { type: "binary"; file: string };

export interface RequestSource {
  spec: string;
  key: string;
  hash: string;
  orphaned: boolean;
}

/** The editable part of a request: what is stored in the `.req.yaml` file. */
export interface RequestDoc {
  /** ULID. Empty string for a brand-new draft; `request_save` assigns one. */
  id: string;
  name: string;
  method: Method;
  url: string;
  pathParams: Record<string, string>;
  query: KeyValue[];
  headers: KeyValue[];
  body: Body;
  auth: Auth;
  timeoutMs?: number;
  followRedirects?: boolean;
  notes?: string;
  source?: RequestSource;
}

/** A request as the panel holds it: the document plus where it lives and its agent-access state. */
export interface RequestDto {
  doc: RequestDoc;
  /** Content hash of the file; `null` for an unsaved draft. Sent back as `expectedRevision` on save. */
  revision: string | null;
  collection: string | null;
  /** Folder slugs from the collection root. */
  folder: string[];
  agentAccess: AgentAccess;
  /** The grant was suspended because the agent edited the request: show "Changed by the agent, review and re-allow". */
  grantSuspended: boolean;
  /** The file is from a newer Hopper: show it, never save it. */
  readOnly: boolean;
}

// ------------------------------------------------------------------------------- workspace and tree

export interface WorkspaceInfo {
  id: string;
  name: string;
  path: string;
}

export interface RecentWorkspace {
  path: string;
  name: string;
  lastOpened: string;
}

export interface DiscoveredWorkspace {
  path: string;
  name: string;
}

export interface SpecLinkDto {
  name: string;
  url: string;
  importedAt?: string;
  operationCount: number;
}

export type TreeNode =
  | { type: "folder"; slug: string; name: string; children: TreeNode[] }
  | {
      type: "request";
      id: string;
      name: string;
      method: Method;
      agentAccess: AgentAccess;
      grantSuspended: boolean;
      orphaned: boolean;
    };

export interface CollectionNode {
  slug: string;
  name: string;
  specs: SpecLinkDto[];
  children: TreeNode[];
}

export interface TreeDto {
  collections: CollectionNode[];
  /** Files that failed to load; shown in the collections sheet so nothing is silently dropped. */
  problems: { path: string; message: string }[];
}

// ------------------------------------------------------------------------------------ environments

export type SecretStatus = "set" | "missing";

export interface EnvVariable {
  name: string;
  value: string;
  /** Value comes from the user's `*.local.yaml` override. */
  local: boolean;
}

export interface PatternRule {
  method?: Method;
  host: string;
  path: string;
}

/** Mirrors `EnvSecretDto` (dto.rs). */
export interface EnvSecretDto {
  name: string;
  provider: string;
  status: SecretStatus;
}

/** Mirrors `ApplicationEnvDto`: one application's section of a stage. */
export interface ApplicationEnvDto {
  application: string;
  variables: EnvVariable[];
  secrets: EnvSecretDto[];
  tls: { insecureSkipVerify: boolean };
  /** Effective list (explicit, or derived from the base-URL variables). */
  allowedHosts: string[];
  allowedHostsExplicit: boolean;
}

/** Mirrors `AgentRulesDto`. */
export interface AgentRulesDto {
  deny: boolean;
  safeReads: boolean;
  patterns: PatternRule[];
}

/** Mirrors `StageDto`: one of the four fixed stages with its application sections. */
export interface StageDto {
  stage: Stage;
  active: boolean;
  applications: ApplicationEnvDto[];
  revision: string | null;
  readOnly: boolean;
  agentRules: AgentRulesDto;
}

export interface NameValue {
  name: string;
  value: string;
}

export interface NameProvider {
  name: string;
  provider: string;
}

/** Mirrors `ApplicationEnvInput`: file-backed fields only. Secret values go through `secret_set`. */
export interface ApplicationEnvInput {
  stage: Stage;
  application: string;
  variables: NameValue[];
  secrets: NameProvider[];
  tls: { insecureSkipVerify: boolean };
  allowedHosts?: string[];
  expectedRevision?: string;
}

/** Mirrors `LegacyEnvDto`: a v1 environment file that still needs migrating. */
export interface LegacyEnvDto {
  stem: string;
  suggestedStage?: Stage;
  suggestedApplication?: string;
  secretNames: string[];
  variableNames: string[];
}

export interface MigrationFailure {
  name: string;
  reason: string;
}

/** Mirrors `MigrationReport`. */
export interface MigrationReport {
  stage: Stage;
  application: string;
  moved: string[];
  failed: MigrationFailure[];
  notSet: string[];
  oldFileKept: boolean;
}

/** Mirrors `VarTargetDto` (internally tagged by `type`). */
export type VarTargetDto =
  | { type: "workspace" }
  | { type: "collection"; slug: string }
  | { type: "folder"; collection: string; path: string[] }
  | { type: "request"; id: string };

/** Mirrors `VariablesDto`. */
export interface VariablesDto {
  target: VarTargetDto;
  variables: NameValue[];
  revision: string | null;
}

/** Mirrors `VariablesInput`. */
export interface VariablesInput {
  target: VarTargetDto;
  variables: NameValue[];
  expectedRevision?: string;
}

/** Mirrors `CollectionAuthDto`. A saved `none` comes back as `inherit`. */
export interface CollectionAuthDto {
  slug: string;
  auth: Auth;
  revision: string | null;
}

/** Mirrors `CollectionAuthInput`. */
export interface CollectionAuthInput {
  slug: string;
  auth: Auth;
  expectedRevision?: string;
}

// ------------------------------------------------------------------------------- spec and search

export type SpecSource = { url: string } | { file: string };

export interface SpecImportResult {
  collection: string;
  spec: string;
  title: string;
  version: string;
  operationCount: number;
}

export interface SearchQuery {
  /** Free words plus the syntax from the plan: `post users`, `tag:Admin`, `#GetUserById`, `path:/api/users/{id}`. */
  query: string;
  collection?: string;
  tag?: string;
  method?: Method;
  limit?: number;
}

export type SearchHit =
  | {
      kind: "saved";
      requestId: string;
      name: string;
      method: Method;
      path: string;
      collection: string;
      /** Character indices of `path` that matched, for highlighting. */
      matches: number[];
      score: number;
      agentAccess: AgentAccess;
    }
  | {
      kind: "operation";
      collection: string;
      spec: string;
      key: string;
      method: Method;
      path: string;
      summary?: string;
      tags: string[];
      deprecated: boolean;
      matches: number[];
      score: number;
      /** Already added to the collection as a saved request. */
      savedRequestId?: string;
    };

export interface SearchResult {
  hits: SearchHit[];
  total: number;
  /** Facets over the full match set, not just the returned page. */
  tags: { name: string; count: number }[];
  methods: { method: Method; count: number }[];
}

// ----------------------------------------------------------------------------------------- sending

export type SendTarget =
  | { requestId: string }
  | { draft: RequestDoc; collection: string | null; folder: string[] };

export interface SendInput {
  /** Chosen by the UI so `request_cancel` can address the run. */
  runId: string;
  target: SendTarget;
  overrides?: Record<string, string>;
  /** Echo of `details.host` from a `confirm_unlisted_host` error, after the user confirmed sending secrets there. */
  confirmedHost?: string;
}

export type ContentKind = "json" | "text" | "html" | "image" | "binary";

export interface Hop {
  status: number;
  url: string;
}

export interface RunResult {
  runId: string;
  historyId: string;
  requestId: string | null;
  env: string;
  status: number;
  statusText: string;
  httpVersion: string;
  headers: [string, string][];
  kind: ContentKind;
  /** Text body (secrets scrubbed). Empty for binary; use `bodyBase64` for images. */
  body: string;
  bodyBase64?: string;
  truncated: boolean;
  /** Where the full body is saved on disk, set when it is larger than history keeps (1 MiB). */
  bodyFile?: string;
  timing: { ttfbMs: number; downloadMs: number; totalMs: number };
  transferredBytes?: number;
  decodedBytes: number;
  redirects: Hop[];
  finalUrl: string;
  insecureTls: boolean;
  /** The URL as sent, with secrets masked. */
  resolvedUrl: string;
}

export interface HistoryItem {
  id: string;
  requestId: string;
  env: string;
  via: "ui" | "mcp";
  sentAt: string;
  method: Method;
  url: string;
  status: number;
  totalMs: number;
  sizeBytes: number;
  /** A name tag: a labelled run is never pruned. */
  label?: string;
  /** The request that was sent is stored, so the run can be reopened. */
  hasRequest: boolean;
}

/** A run in the workspace-wide history list. */
export interface HistoryRow extends HistoryItem {
  /** The saved request's name, or the host and path for a run without one. */
  name: string;
  /** The request still exists in a collection. */
  saved: boolean;
}

/** The request stored with a run, as it was written (variables unresolved). */
export interface HistoryRequest {
  doc: RequestDoc;
  /** The body was left out because it was too large to keep. */
  truncated: boolean;
}

export interface CookieDto {
  name: string;
  value: string;
  domain: string;
  path: string;
  secure: boolean;
  httpOnly: boolean;
  expires?: string;
}

// ----------------------------------------------------------------------------------- agent and MCP

export interface MaskedRequest {
  method: Method;
  url: string;
  headers: [string, string][];
  bodyPreview?: string;
}

export interface ApprovalRequest {
  id: string;
  client: string;
  requestName: string;
  requestId: string | null;
  stage: Stage;
  application: string;
  request: MaskedRequest;
  /** Why it asks, e.g. "no grant", "grant suspended: changed by the agent", "inline request". */
  reason: string;
  createdAt: string;
  /** The prompt ignores input until this many ms after it appeared (keystroke-capture guard). */
  armAfterMs: number;
}

export type ApprovalDecision = "allow_once" | "allow_session" | "always_allow" | "block" | "deny";

/** What an agent just added: one request (`kind: "request"`) or the operations of an imported spec. */
export interface EndpointsAdded {
  client: string;
  collection: string;
  kind: "request" | "spec";
  /** `POST /users` for a request, the spec title for an import. */
  label: string;
  count: number;
  /** The new request, so the notice can open it. */
  requestId?: string;
}

export interface ActivityEntry {
  id: string;
  at: string;
  client: string;
  tool: string;
  target: string;
  outcome: "ok" | "denied" | "not_approved" | "error";
  detail?: string;
  /** For sends: "local environment" | "request grant" | "pattern rule" | "approved by user". */
  allowedBy?: string;
}

export interface GrantDto {
  requestId: string;
  requestName: string;
  access: AgentAccess;
  suspended: boolean;
}

export interface McpStatus {
  running: boolean;
  port?: number;
  url?: string;
  error?: string;
}

/** `stale`: Claude Code has a `hopper` entry whose URL or token no longer matches this server. */
export type ClaudeConnection = "none" | "connected" | "stale";

export interface McpSetup {
  claudeConnection: ClaudeConnection;
  /** `claude mcp add ...` with the bearer token, for copy-paste. Contains the MCP token, not a user secret. */
  claudeCommand: string;
  jsonConfig: string;
}

// ------------------------------------------------------------------------------------------ settings

export interface Settings {
  hotkey: string;
  dockSide: "left" | "right";
  widthPreset: 430 | 640 | 860;
  theme: "dark" | "light" | "system";
  alwaysShowTab: boolean;
  startWithWindows: boolean | null;
}

export interface RecentItem {
  id: string;
  name: string;
  method: Method;
  status: number | null;
}

// -------------------------------------------------------------------------------- commands and events

export interface CommandMap {
  // workspace
  workspace_current: { args: undefined; result: WorkspaceInfo | null };
  workspace_recent: { args: undefined; result: RecentWorkspace[] };
  workspace_known: { args: undefined; result: DiscoveredWorkspace[] };
  workspace_discover: { args: undefined; result: DiscoveredWorkspace[] };
  workspace_open: { args: { path: string }; result: WorkspaceInfo };
  workspace_init: { args: { path: string; name: string }; result: WorkspaceInfo };
  workspace_close: { args: undefined; result: void };
  /** Opens the native folder picker (the only native dialog allowed). `null` when cancelled. */
  workspace_pick_folder: { args: undefined; result: string | null };
  tree_get: { args: undefined; result: TreeDto };
  collection_create: { args: { name: string }; result: string };
  folder_create: { args: { collection: string; parent: string[]; name: string }; result: string };
  recent_list: { args: undefined; result: RecentItem[] };

  // requests
  request_get: { args: { id: string }; result: RequestDto };
  request_save: {
    args: { doc: RequestDoc; collection: string; folder: string[]; expectedRevision?: string };
    result: RequestDto;
  };
  request_delete: { args: { id: string }; result: { undoToken: string } };
  request_undelete: { args: { undoToken: string }; result: RequestDto };
  request_move: { args: { id: string; collection: string; folder: string[] }; result: RequestDto };
  request_duplicate: { args: { id: string }; result: RequestDto };
  /** Builds an unsaved draft from a spec operation. */
  operation_open: { args: { collection: string; spec: string; key: string }; result: RequestDto };

  // environments and secrets
  environment_list: { args: undefined; result: StageDto[] };
  environment_save: { args: { env: ApplicationEnvInput }; result: StageDto };
  environment_delete: { args: { stage: Stage; application: string }; result: void };
  environment_set_active: { args: { stage: Stage | null }; result: void };
  environment_legacy_list: { args: undefined; result: LegacyEnvDto[] };
  environment_migrate_legacy: { args: { stem: string; stage: Stage; application: string }; result: MigrationReport };
  variables_get: { args: { target: VarTargetDto }; result: VariablesDto };
  variables_save: { args: { input: VariablesInput }; result: VariablesDto };
  collection_auth_get: { args: { slug: string }; result: CollectionAuthDto };
  collection_auth_save: { args: { input: CollectionAuthInput }; result: CollectionAuthDto };
  secret_set: { args: { stage: Stage; application: string; name: string; value: string }; result: void };
  secret_clear: { args: { stage: Stage; application: string; name: string }; result: void };
  /** Explicit, in-app Reveal. The UI hides the value again after 10 s. */
  secret_reveal: { args: { stage: Stage; application: string; name: string }; result: string };

  // specs and search
  spec_import: { args: { collection: string; source: SpecSource; name?: string }; result: SpecImportResult };
  search_routes: { args: SearchQuery; result: SearchResult };

  // sending
  request_send: { args: SendInput; result: RunResult };
  request_cancel: { args: { runId: string }; result: void };
  copy_curl: { args: { target: SendTarget; masked: boolean }; result: string };
  history_list: { args: { requestId: string }; result: HistoryItem[] };
  history_get: { args: { requestId: string; id: string }; result: RunResult };
  history_all: { args: undefined; result: HistoryRow[] };
  /** `null` or an empty label removes it. */
  history_label: { args: { requestId: string; id: string; label: string | null }; result: void };
  history_delete: { args: { requestId: string; id: string }; result: void };
  history_request: { args: { requestId: string; id: string }; result: HistoryRequest };
  history_clear: { args: { requestId?: string }; result: void };
  cookies_list: { args: undefined; result: CookieDto[] };
  cookies_clear: { args: undefined; result: void };

  // agent
  agent_access_set: { args: { requestIds: string[]; access: AgentAccess }; result: void };
  agent_grants_list: { args: undefined; result: GrantDto[] };
  agent_env_rules_set: {
    args: { env: string; deny: boolean; safeReads: boolean; patterns: PatternRule[] };
    result: void;
  };
  agent_activity_list: { args: undefined; result: ActivityEntry[] };
  approval_pending: { args: undefined; result: ApprovalRequest[] };
  approval_respond: { args: { id: string; decision: ApprovalDecision }; result: void };
  mcp_status: { args: undefined; result: McpStatus };
  mcp_setup: { args: undefined; result: McpSetup };
  /** Runs `claude mcp add ...` for the user; resolves with the CLI's output. */
  mcp_connect_claude: { args: undefined; result: string };

  // settings
  settings_get: { args: undefined; result: Settings };
  settings_set: { args: { settings: Settings }; result: Settings };
}

export type CommandName = keyof CommandMap;

/** Payloads of the events the backend emits (`@tauri-apps/api/event`). */
export interface EventMap {
  "workspace://changed": undefined;
  "run://progress": { runId: string; phase: "preparing" | "sending" | "receiving" | "done" };
  "secrets://status": { stage: Stage; application: string };
  "agent://activity": ActivityEntry;
  "agent://endpoints-added": EndpointsAdded;
  "agent://approval-request": ApprovalRequest;
  "agent://approval-resolved": { id: string };
  "mcp://status": McpStatus;
  "panel://show-request": { id: string };
  /** Tray "Check for updates": the panel runs a check and shows the answer in Settings. */
  "update://check": undefined;
  /** Tray Settings (and similar): the panel opens this sheet. */
  "panel://open-sheet": { sheet: "settings" | "environments" | "agent" | "collections" };
}

export type EventName = keyof EventMap;
