/* Dev-only stand-in for the Tauri backend, so the whole panel runs in a plain browser (`pnpm dev`) and in Vitest.
   Every command in `CommandMap` has a handler here (the `Handlers` type enforces it). Seed data, search and send live in ./mock/*.
   In the browser, `window.__hopperMock` triggers approvals, activity and MCP changes. */
import { bus } from "../bus";
import type {
  ActivityEntry,
  EndpointsAdded,
  ApprovalRequest,
  CommandMap,
  CommandName,
  ApplicationEnvDto,
  Stage,
  HistoryItem,
  McpStatus,
  RequestDto,
  RunResult,
  SendTarget,
  TreeNode,
  TreeDto,
  RecentItem,
  RequestDoc,
  VariablesDto,
  Auth,
} from "../types";
import {
  COLLECTION,
  COLLECTION_NAME,
  SPEC,
  cookies as seedCookies,
  operations,
  recentWorkspaces,
  seedActivity,
  seedEnvironments,
  seedRequests,
  settings as seedSettings,
  workspace as seedWorkspace,
} from "./mock/seed";
import { searchRoutes } from "./mock/search";
import { cancelRun, err, resolve, simulateSend } from "./mock/send";

type Handlers = { [K in CommandName]: (args: CommandMap[K]["args"]) => CommandMap[K]["result"] | Promise<CommandMap[K]["result"]> };

interface HistoryEntry {
  item: HistoryItem;
  run: RunResult;
}

let uid = 0;
const nextId = (prefix: string) => `${prefix}${Date.now().toString(36)}${(++uid).toString(36)}`.toUpperCase();
const clone = <T>(v: T): T => structuredClone(v);
const titleCase = (slug: string) => slug.replace(/-/g, " ").replace(/^\w/, (c) => c.toUpperCase());

function fresh() {
  const requests = new Map(seedRequests().map((r) => [r.doc.id, r]));
  const s = {
    workspace: clone(seedWorkspace) as typeof seedWorkspace | null,
    requests,
    folders: new Set<string>(),
    trash: new Map<string, RequestDto>(),
    envs: seedEnvironments(),
    settings: clone(seedSettings),
    cookies: clone(seedCookies),
    history: new Map<string, HistoryEntry[]>(),
    activity: seedActivity(),
    approvals: [] as ApprovalRequest[],
    mcp: { running: true, port: 47821, url: "http://127.0.0.1:47821/mcp" } as McpStatus,
    claudeConnected: false,
    scopeVars: {} as Record<string, VariablesDto>,
    collectionAuth: {} as Record<string, { auth: Auth; rev: string }>,
    revision: 1,
    latency: typeof window === "undefined" ? 0 : 40,
  };
  const t = Date.now();
  const seedRuns: [string, number, number, number][] = [
    ["01HX0000000000000000000002", 200, 142, t - 3 * 60_000],
    ["01HX0000000000000000000001", 200, 311, t - 9 * 60_000],
    ["01HX0000000000000000000003", 503, 1204, t - 55 * 60_000],
    ["01HX0000000000000000000007", 404, 98, t - 3 * 3_600_000],
  ];
  for (const [id, status, ms, at] of seedRuns) {
    const r = requests.get(id)!;
    const run = fakeRun(r.doc, status, ms);
    pushHistory(s.history, id, run, r.doc, at);
  }
  return s;
}

function fakeRun(doc: RequestDoc, status: number, ms: number): RunResult {
  const url = doc.url.replace("{{baseUrl}}", "http://localhost:5080");
  const body = status === 200 ? '{\n  "ok": true\n}' : JSON.stringify({ title: status === 404 ? "Not Found" : "Service Unavailable", status }, null, 2);
  return {
    runId: nextId("run"), historyId: nextId("hist"), requestId: doc.id, env: "local", status,
    statusText: status === 200 ? "OK" : status === 404 ? "Not Found" : "Service Unavailable",
    httpVersion: "HTTP/1.1", headers: [["content-type", "application/json"]], kind: "json", body, truncated: false,
    timing: { ttfbMs: Math.round(ms * 0.7), downloadMs: Math.round(ms * 0.3), totalMs: ms },
    decodedBytes: body.length, redirects: [], finalUrl: url, insecureTls: false, resolvedUrl: url,
  };
}

function pushHistory(history: Map<string, HistoryEntry[]>, requestId: string, run: RunResult, doc: RequestDoc, at = Date.now()) {
  const item: HistoryItem = {
    id: run.historyId, requestId, env: run.env, via: "ui", sentAt: new Date(at).toISOString(),
    method: doc.method, url: run.resolvedUrl, status: run.status, totalMs: run.timing.totalMs, sizeBytes: run.decodedBytes,
  };
  history.set(requestId, [{ item, run }, ...(history.get(requestId) ?? [])]);
}

let s = fresh();

/** The mock's single application section: the active stage's `account` section, named by stage. */
const ACCOUNT = "account";
const activeEnv = () => {
  const st = s.envs.find((e) => e.active);
  const sec = st?.applications.find((a) => a.application === ACCOUNT);
  return st && sec ? { name: st.stage, ...sec } : undefined;
};
/** Looks up a secret by stage and application; throws `not_found` like the Rust service. */
const findSecret = (stage: Stage, application: string, name: string) => {
  const sec = s.envs.find((e) => e.stage === stage)?.applications.find((a) => a.application === application)?.secrets.find((x) => x.name === name);
  if (!sec) throw err("not_found", `No secret ${name} in ${stage}/${application}.`);
  return sec;
};
const getReq = (id: string): RequestDto => {
  const r = s.requests.get(id);
  if (!r) throw err("not_found", `No request ${id}.`);
  return clone(r);
};
const bump = (r: RequestDto) => {
  r.revision = `rev-${r.doc.id}-${++s.revision}`;
};

function buildTree(): TreeDto {
  const root: TreeNode[] = [];
  const folderOf = (path: string[]): TreeNode[] => {
    let level = root;
    for (const slug of path) {
      let f = level.find((n) => n.type === "folder" && n.slug === slug);
      if (!f) level.push((f = { type: "folder", slug, name: titleCase(slug), children: [] }));
      level = (f as Extract<TreeNode, { type: "folder" }>).children;
    }
    return level;
  };
  for (const f of s.folders) folderOf(f.split("/"));
  for (const r of s.requests.values()) {
    folderOf(r.folder).push({
      type: "request", id: r.doc.id, name: r.doc.name, method: r.doc.method, agentAccess: r.agentAccess,
      grantSuspended: r.grantSuspended, orphaned: !!r.doc.source?.orphaned,
    });
  }
  const sort = (nodes: TreeNode[]) => {
    nodes.sort((a, b) => (a.type === b.type ? a.name.localeCompare(b.name) : a.type === "folder" ? -1 : 1));
    nodes.forEach((n) => n.type === "folder" && sort(n.children));
  };
  sort(root);
  const specCount = operations.length;
  return {
    collections: [{ slug: COLLECTION, name: COLLECTION_NAME, specs: [{ name: SPEC, url: "https://account.umbler.dev/swagger/v1/swagger.json", importedAt: "2026-10-20T14:00:00Z", operationCount: specCount }], children: root }],
    problems: [],
  };
}

function docOf(target: SendTarget): { doc: RequestDoc; requestId: string | null } {
  if ("requestId" in target) return { doc: getReq(target.requestId).doc, requestId: target.requestId };
  return { doc: target.draft, requestId: target.draft.id && s.requests.has(target.draft.id) ? target.draft.id : null };
}

function curl(target: SendTarget, masked: boolean): string {
  const { doc } = docOf(target);
  const env = activeEnv();
  const { url } = resolve(doc, env);
  const parts = [`curl -X ${doc.method} '${url}'`];
  doc.headers.filter((h) => h.enabled).forEach((h) => parts.push(`-H '${h.name}: ${h.value}'`));
  if (doc.auth.type === "bearer") parts.push(`-H 'Authorization: Bearer ${masked ? "\u2022\u2022\u2022\u2022" : "mock-token"}'`);
  if (doc.body.type === "json" || doc.body.type === "text" || doc.body.type === "xml") parts.push(`--data '${doc.body.content.replace(/\n\s*/g, " ")}'`);
  return parts.join(" \\\n  ");
}

function deriveHosts(vars: { name: string; value: string }[]): string[] {
  return vars.flatMap((v) => (/url$/i.test(v.name) ? [v.value.replace(/^https?:\/\//, "").split(/[/:]/)[0]] : []));
}

const handlers: Handlers = {
  workspace_current: () => clone(s.workspace),
  workspace_recent: () => clone(recentWorkspaces),
  workspace_known: () => [],
  workspace_discover: () => [
    { path: "C:\Users\you\Documents\billing-apis", name: "Billing APIs" },
    { path: "C:\Users\you\Downloads\partner-sandbox", name: "Partner sandbox" },
  ],
  workspace_open: ({ path }) => {
    const known = recentWorkspaces.find((w) => w.path === path);
    s.workspace = { id: "ws-" + path.length, name: known?.name ?? path.split(/[\\/]/).pop() ?? path, path };
    bus.emit("workspace://changed");
    return clone(s.workspace);
  },
  workspace_init: ({ path, name }) => {
    s.workspace = { id: "ws-new", name, path };
    bus.emit("workspace://changed");
    return clone(s.workspace);
  },
  workspace_close: () => {
    s.workspace = null;
    bus.emit("workspace://changed");
  },
  workspace_pick_folder: () => "C:\\Work\\umbler-apis",
  tree_get: () => buildTree(),
  collection_create: ({ name }) => name.toLowerCase().replace(/\s+/g, "-"),
  folder_create: ({ parent, name }) => {
    const slug = name.toLowerCase().replace(/\s+/g, "-");
    s.folders.add([...parent, slug].join("/"));
    bus.emit("workspace://changed");
    return slug;
  },
  recent_list: () => {
    const all = [...s.history.values()].flat().sort((a, b) => b.item.sentAt.localeCompare(a.item.sentAt));
    const seen = new Set<string>();
    const out: RecentItem[] = [];
    for (const { item } of all) {
      const r = s.requests.get(item.requestId);
      if (!r || seen.has(r.doc.id)) continue;
      seen.add(r.doc.id);
      out.push({ id: r.doc.id, name: r.doc.name, method: r.doc.method, status: item.status });
    }
    return out.slice(0, 8);
  },

  request_get: ({ id }) => getReq(id),
  request_save: ({ doc, collection, folder, expectedRevision }) => {
    const existing = doc.id ? s.requests.get(doc.id) : undefined;
    if (existing?.readOnly) throw err("read_only", "This file is from a newer Hopper and cannot be saved.");
    if (existing && expectedRevision !== undefined && existing.revision !== expectedRevision) {
      throw err("revision_conflict", "The request changed on disk since you opened it.", { hint: "Reload it or keep your version.", details: { current: existing.revision } });
    }
    const next: RequestDto = existing
      ? { ...existing, doc: clone(doc), collection, folder }
      : { doc: { ...clone(doc), id: doc.id || nextId("01HX") }, revision: null, collection, folder, agentAccess: "ask", grantSuspended: false, readOnly: false };
    bump(next);
    s.requests.set(next.doc.id, next);
    bus.emit("workspace://changed");
    return clone(next);
  },
  request_delete: ({ id }) => {
    const r = getReq(id);
    s.requests.delete(id);
    const undoToken = nextId("undo");
    s.trash.set(undoToken, r);
    bus.emit("workspace://changed");
    return { undoToken };
  },
  request_undelete: ({ undoToken }) => {
    const r = s.trash.get(undoToken);
    if (!r) throw err("not_found", "Nothing to restore.");
    s.trash.delete(undoToken);
    s.requests.set(r.doc.id, r);
    bus.emit("workspace://changed");
    return clone(r);
  },
  request_move: ({ id, collection, folder }) => {
    const r = getReq(id);
    r.collection = collection;
    r.folder = folder;
    bump(r);
    s.requests.set(id, r);
    bus.emit("workspace://changed");
    return clone(r);
  },
  request_duplicate: ({ id }) => {
    const r = getReq(id);
    r.doc.id = nextId("01HX");
    r.doc.name = `${r.doc.name} copy`;
    r.agentAccess = "ask";
    r.grantSuspended = false;
    bump(r);
    s.requests.set(r.doc.id, r);
    bus.emit("workspace://changed");
    return clone(r);
  },
  operation_open: ({ collection, spec, key }) => {
    const op = operations.find((o) => o.key === key);
    if (!op) throw err("not_found", `No operation ${key} in ${spec}.`);
    const pathParams = Object.fromEntries([...op.path.matchAll(/\{(\w+)\}/g)].map((m) => [m[1], ""]));
    const hasBody = ["POST", "PUT", "PATCH"].includes(op.method);
    return {
      doc: {
        id: "", name: op.summary, method: op.method, url: `{{baseUrl}}${op.path}`, pathParams, query: [],
        headers: hasBody ? [{ name: "Content-Type", value: "application/json", enabled: true }] : [],
        body: hasBody ? { type: "json", content: "{\n  \n}" } : { type: "none" },
        auth: { type: "inherit" },
        source: { spec, key, hash: `h-${key}`, orphaned: false },
      },
      revision: null, collection, folder: [], agentAccess: "ask", grantSuspended: false, readOnly: false,
    };
  },

  environment_list: () => clone(s.envs),
  environment_save: ({ env }) => {
    const stage = s.envs.find((e) => e.stage === env.stage);
    if (!stage) throw err("invalid_input", `No stage ${env.stage}.`);
    if (env.expectedRevision !== undefined && stage.revision !== env.expectedRevision) {
      throw err("revision_conflict", "The environment changed on disk since you opened it.");
    }
    const old = stage.applications.find((a) => a.application === env.application);
    const next: ApplicationEnvDto = {
      application: env.application,
      variables: env.variables.map((v) => ({ ...v, local: old?.variables.find((o) => o.name === v.name)?.local ?? false })),
      secrets: env.secrets.map((x) => ({ ...x, status: old?.secrets.find((o) => o.name === x.name)?.status ?? "missing" })),
      tls: env.tls,
      allowedHosts: env.allowedHosts ?? deriveHosts(env.variables),
      allowedHostsExplicit: env.allowedHosts !== undefined,
    };
    stage.applications = [...stage.applications.filter((a) => a.application !== env.application), next];
    stage.revision = `env-${env.stage}-${++s.revision}`;
    return clone(stage);
  },
  environment_delete: ({ stage, application }) => {
    const st = s.envs.find((e) => e.stage === stage);
    if (st) st.applications = st.applications.filter((a) => a.application !== application);
  },
  environment_set_active: ({ stage }) => {
    s.envs.forEach((e) => (e.active = e.stage === stage));
  },
  environment_legacy_list: () => [],
  environment_migrate_legacy: ({ stage, application }) => ({ stage, application, moved: [], failed: [], notSet: [], oldFileKept: true }),
  variables_get: ({ target }) => {
    const at = s.scopeVars[JSON.stringify(target)];
    return { target, variables: clone(at?.variables ?? []), revision: at?.revision ?? "vars-0" };
  },
  variables_save: ({ input }) => {
    const key = JSON.stringify(input.target);
    const at = s.scopeVars[key];
    if (input.expectedRevision !== undefined && input.expectedRevision !== (at?.revision ?? "vars-0")) {
      throw err("revision_conflict", "The variables changed on disk since you opened them.");
    }
    const saved = { target: input.target, variables: clone(input.variables), revision: `vars-${++s.revision}` };
    s.scopeVars[key] = saved;
    return clone(saved);
  },
  collection_auth_get: ({ slug }) => {
    const at = s.collectionAuth[slug];
    const auth: Auth = at?.auth ?? { type: "inherit" };
    return { slug, auth: clone(auth), revision: at?.rev ?? "auth-0" };
  },
  collection_auth_save: ({ input }) => {
    const at = s.collectionAuth[input.slug];
    if (input.expectedRevision !== undefined && input.expectedRevision !== (at?.rev ?? "auth-0")) {
      throw err("revision_conflict", "The collection auth changed on disk since you opened it.");
    }
    const auth: Auth = input.auth.type === "none" ? { type: "inherit" } : clone(input.auth);
    const rev = `auth-${++s.revision}`;
    s.collectionAuth[input.slug] = { auth, rev };
    return { slug: input.slug, auth: clone(auth), revision: rev };
  },
  secret_set: ({ stage, application, name }) => {
    const sec = findSecret(stage, application, name);
    sec.status = "set";
    bus.emit("secrets://status", { stage, application });
  },
  secret_clear: ({ stage, application, name }) => {
    const sec = findSecret(stage, application, name);
    sec.status = "missing";
    bus.emit("secrets://status", { stage, application });
  },
  secret_reveal: ({ stage, application, name }) => {
    const sec = findSecret(stage, application, name);
    if (sec.status !== "set") throw err("secret_missing", `${name} has no value in ${stage}.`);
    return "mock-secret-value-7f3a9c";
  },

  spec_import: ({ collection, source, name }) => {
    if ("url" in source && !/^https?:\/\//.test(source.url)) throw err("invalid_input", "The spec URL must start with http:// or https://.");
    return { collection, spec: name ?? SPEC, title: "Account API", version: "v1", operationCount: operations.length };
  },
  search_routes: (q) => searchRoutes(q, operations, [...s.requests.values()]),

  request_send: async (input) => {
    const { doc, requestId } = docOf(input.target);
    const run = await simulateSend(input, doc, activeEnv(), requestId);
    if (requestId) pushHistory(s.history, requestId, run, doc);
    return run;
  },
  request_cancel: ({ runId }) => cancelRun(runId),
  copy_curl: ({ target, masked }) => curl(target, masked),
  history_list: ({ requestId }) => (s.history.get(requestId) ?? []).map((h) => clone(h.item)),
  history_get: ({ requestId, id }) => {
    const h = s.history.get(requestId)?.find((x) => x.item.id === id);
    if (!h) throw err("not_found", "That history entry is gone.");
    return clone(h.run);
  },
  history_clear: ({ requestId }) => {
    if (requestId) s.history.delete(requestId);
    else s.history.clear();
  },
  cookies_list: () => clone(s.cookies),
  cookies_clear: () => {
    s.cookies = [];
  },

  agent_access_set: ({ requestIds, access }) => {
    for (const id of requestIds) {
      const r = s.requests.get(id);
      if (!r) continue;
      r.agentAccess = access;
      r.grantSuspended = false;
    }
    bus.emit("workspace://changed");
  },
  agent_grants_list: () =>
    [...s.requests.values()]
      .filter((r) => r.agentAccess !== "ask" || r.grantSuspended)
      .map((r) => ({ requestId: r.doc.id, requestName: r.doc.name, access: r.agentAccess, suspended: r.grantSuspended })),
  agent_env_rules_set: ({ env, deny, safeReads, patterns }) => {
    const e = s.envs.find((x) => x.stage === env);
    if (!e) throw err("not_found", `No stage ${env}.`);
    e.agentRules = { deny, safeReads, patterns };
  },
  agent_activity_list: () => {
    if (!s.workspace) throw err("no_workspace", "No workspace is open.");
    return clone(s.activity);
  },
  approval_pending: () => {
    if (!s.workspace) throw err("no_workspace", "No workspace is open.");
    return clone(s.approvals);
  },
  approval_respond: ({ id, decision }) => {
    const a = s.approvals.find((x) => x.id === id);
    if (!a) throw err("not_found", "That approval is no longer pending.");
    s.approvals = s.approvals.filter((x) => x.id !== id);
    if (decision === "always_allow" && a.requestId) {
      const r = s.requests.get(a.requestId);
      if (r) Object.assign(r, { agentAccess: "auto", grantSuspended: false });
    }
    if (decision === "block" && a.requestId) {
      const r = s.requests.get(a.requestId);
      if (r) r.agentAccess = "blocked";
    }
    const denied = decision === "block" || decision === "deny";
    pushActivity({ client: a.client, tool: "hopper_send", target: a.requestName, outcome: denied ? "denied" : "ok", allowedBy: denied ? undefined : "approved by user" });
    bus.emit("agent://approval-resolved", { id });
  },
  mcp_status: () => clone(s.mcp),
  mcp_setup: () => ({
    claudeConnection: s.claudeConnected ? "connected" : "none",
    claudeCommand: 'claude mcp add --transport http hopper http://127.0.0.1:47821/mcp --header "Authorization: Bearer hpr_mock_token"',
    jsonConfig: JSON.stringify({ mcpServers: { hopper: { type: "http", url: "http://127.0.0.1:47821/mcp", headers: { Authorization: "Bearer hpr_mock_token" } } } }, null, 2),
  }),
  mcp_connect_claude: () => {
    s.claudeConnected = true;
    return "Added HTTP MCP server hopper to local config";
  },

  settings_get: () => clone(s.settings),
  settings_set: ({ settings }) => {
    s.settings = clone(settings);
    return clone(s.settings);
  },
};

function pushActivity(partial: Omit<ActivityEntry, "id" | "at">) {
  const entry: ActivityEntry = { id: nextId("act"), at: new Date().toISOString(), ...partial };
  s.activity = [entry, ...s.activity];
  bus.emit("agent://activity", entry);
  return entry;
}

/** Answers a command the way the Rust side would, without a backend. */
export async function mockInvoke<K extends CommandName>(cmd: K, args?: CommandMap[K]["args"]): Promise<CommandMap[K]["result"]> {
  const handler = handlers[cmd] as ((a: CommandMap[K]["args"]) => CommandMap[K]["result"] | Promise<CommandMap[K]["result"]>) | undefined;
  if (!handler) throw err("internal", `The mock has no handler for ${cmd}.`);
  if (s.latency > 0) await new Promise((r) => setTimeout(r, s.latency));
  return handler(args as CommandMap[K]["args"]);
}

/** Browser console helpers: `__hopperMock.requestApproval()`, `.activity()`, `.setMcp({ running: false })`, `.reset()`. */
export const hopperMock = {
  reset: () => void (s = fresh()),
  setLatency: (ms: number) => void (s.latency = ms),
  /** A pending approval for a production send, the way an agent would trigger one. */
  requestApproval(over: Partial<ApprovalRequest> = {}): ApprovalRequest {
    const a: ApprovalRequest = {
      id: nextId("apr"), client: "Claude Code", requestName: "Reset password", requestId: "01HX0000000000000000000004",
      stage: "production", application: "account",
      request: {
        method: "POST", url: "https://account.umbler.com/api/users/9001/password-reset",
        headers: [["Authorization", "Bearer \u2022\u2022\u2022\u2022"], ["Content-Type", "application/json"]],
        bodyPreview: '{ "notify": true }',
      },
      reason: "grant suspended: changed by the agent", createdAt: new Date().toISOString(), armAfterMs: 500, ...over,
    };
    s.approvals.push(a);
    bus.emit("agent://approval-request", a);
    return a;
  },
  activity: (over: Partial<Omit<ActivityEntry, "id" | "at">> = {}) =>
    pushActivity({ client: "Claude Code", tool: "hopper_search_routes", target: "user sessions", outcome: "ok", ...over }),
  /** An agent adding an endpoint, the way `hopper_create_request` does: the floating Hopper announces it. */
  endpoints: (over: Partial<EndpointsAdded> = {}) =>
    bus.emit("agent://endpoints-added", { client: "Claude Code", collection: "account", kind: "request", label: "POST /api/users", count: 1, ...over }),
  /** Flips the MCP status; there is no MCP event in the contract, so this also emits an activity entry that makes the store re-read it. */
  setMcp(status: McpStatus) {
    s.mcp = status;
    pushActivity({ client: "Hopper", tool: "mcp_status", target: status.running ? "running" : "stopped", outcome: status.error ? "error" : "ok" });
  },
  emit: bus.emit.bind(bus),
};

declare global {
  interface Window {
    __hopperMock?: typeof hopperMock;
  }
}
if (typeof window !== "undefined") window.__hopperMock = hopperMock;
