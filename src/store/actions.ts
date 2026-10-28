/* Actions: plain functions that call `invoke` and update the store. Rust owns the truth; the store is a cache. */
import {
  invoke,
  listen,
  isHopperError,
  toHopperError,
  type ApprovalDecision,
  type HopperError,
  type Method,
  type RequestDoc,
  type RunResult,
  type Settings,
} from "../ipc";
import { checkForUpdates } from "../updater";
import { debounced, latestWins } from "./latest";
import {
  draftFromDto,
  draftMeetsDisk,
  draftSaved,
  applyEdit,
  applyMove,
  newRun,
  pruneRuns,
  runFailed,
  runFinished,
  runProgress,
  type DraftState,
  type Route,
  type AgentTab,
  type SheetName,
} from "./reducers";
import { scopeTargets, targetKey, withVariable } from "./scopes";
import { initialSearch, initialState, store, type Toast, type ToastKind } from "./state";
import type { Auth, CollectionAuthDto, NameValue, Stage, VarTargetDto, VariablesDto } from "../ipc/types";
import { confirm } from "../ui/overlays";

const { getState, setState } = store;

export const SEARCH_DEBOUNCE_MS = 120;
export const ACTIVITY_CAP = 200;
export const NOTICE_CAP = 20;

/** Runs a command and shows an in-app error toast if it fails; returns `undefined` on failure. */
async function guarded<T>(work: () => Promise<T>): Promise<T | undefined> {
  try {
    return await work();
  } catch (e) {
    toast(toHopperError(e).message, "error");
    return undefined;
  }
}

// ----------------------------------------------------------------------------------------- toasts

let toastSeq = 0;

export function toast(message: string, kind: ToastKind = "info", extra: Partial<Pick<Toast, "undo" | "ttl">> = {}): string {
  const id = `toast-${++toastSeq}`;
  const ttl = extra.ttl ?? (kind === "error" ? 8000 : extra.undo ? 10000 : 3500);
  setState((s) => ({ toasts: [...s.toasts, { id, kind, message, ttl, undo: extra.undo }] }));
  return id;
}

export function dismissToast(id: string) {
  setState((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) }));
}

export function dismissError() {
  setState({ error: null });
}

// ---------------------------------------------------------------------------------- navigation

export const setRoute = (route: Route) => setState({ route });
export const openSheet = (sheet: SheetName) => setState({ sheet, agentTab: null });
export const openAgentSheet = (agentTab: AgentTab) => setState({ sheet: "agent", agentTab });
export const closeSheet = () => setState({ sheet: null });

// -------------------------------------------------------------------------------------- workspace

export async function loadWorkspace() {
  const [workspace, recentWorkspaces] = await Promise.all([invoke("workspace_current"), invoke("workspace_recent")]);
  setState({ workspace, recentWorkspaces });
  if (workspace) await Promise.all([refreshTree(), refreshRecent(), refreshEnvironments()]);
  else setState({ tree: null, recent: [], environments: [] });
}

export async function openWorkspace(path: string) {
  const ws = await guarded(() => invoke("workspace_open", { path }));
  if (!ws) return;
  setState({ draft: null, route: "find", runs: {}, history: {}, search: initialSearch });
  await loadWorkspace();
}

export async function initWorkspace(path: string, name: string) {
  const ws = await guarded(() => invoke("workspace_init", { path, name }));
  if (ws) await loadWorkspace();
}

/** Opens the native folder picker, then the chosen folder. */
export async function pickAndOpenWorkspace() {
  const path = await guarded(() => invoke("workspace_pick_folder"));
  if (path) await openWorkspace(path);
}

export async function closeWorkspace() {
  await guarded(() => invoke("workspace_close"));
  setState({ ...pick(initialState(), ["draft", "route", "runs", "history", "search", "tree", "recent", "environments", "workspace"]), sheet: null });
  setState({ recentWorkspaces: await invoke("workspace_recent") });
}

function pick<T extends object, K extends keyof T>(o: T, keys: K[]): Pick<T, K> {
  return Object.fromEntries(keys.map((k) => [k, o[k]])) as Pick<T, K>;
}

export async function refreshTree() {
  const tree = await guarded(() => invoke("tree_get"));
  if (tree) setState({ tree });
}

export async function refreshRecent() {
  const recent = await guarded(() => invoke("recent_list"));
  if (recent) setState({ recent });
}

export async function createCollection(name: string): Promise<string | undefined> {
  const slug = await guarded(() => invoke("collection_create", { name }));
  await refreshTree();
  return slug;
}

export async function createFolder(collection: string, parent: string[], name: string) {
  const slug = await guarded(() => invoke("folder_create", { collection, parent, name }));
  await refreshTree();
  return slug;
}

// ------------------------------------------------------------------------------------ environments

export async function refreshEnvironments() {
  const environments = await guarded(() => invoke("environment_list"));
  if (environments) setState({ environments });
}

/** Switches the active environment; optimistic, and re-read from the backend afterwards. */
export async function setActiveEnv(stage: Stage | null) {
  setState((s) => ({ environments: s.environments.map((e) => ({ ...e, active: e.stage === stage })) }));
  await guarded(() => invoke("environment_set_active", { stage }));
  await refreshEnvironments();
}

// ------------------------------------------------------------------------------- scoped variables

/** Reads one scope's variables into the store. Quiet on failure: a missing request or collection just has none. */
export async function loadVariables(target: VarTargetDto): Promise<VariablesDto | undefined> {
  try {
    const dto = await invoke("variables_get", { target });
    setState((s) => ({ scopeVars: { ...s.scopeVars, [targetKey(target)]: dto } }));
    return dto;
  } catch {
    return undefined;
  }
}

/** Re-reads every scope the open request resolves against: workspace, collection, folders, the request. */
export async function refreshScopeVars() {
  const d = getState().draft;
  if (!d) return;
  const saved = d.base.revision !== null && d.doc.id ? d.doc.id : null;
  await Promise.all(scopeTargets(d.base.collection, d.base.folder, saved).map((t) => loadVariables(t.target)));
}

/** Replaces one scope's variables; a failure shows as a toast. `expectedRevision` guards against a stale overwrite. */
export async function saveVariables(target: VarTargetDto, variables: NameValue[], expectedRevision: string | null): Promise<boolean> {
  const saved = await guarded(() => invoke("variables_save", { input: { target, variables, expectedRevision: expectedRevision ?? undefined } }));
  if (saved) setState((s) => ({ scopeVars: { ...s.scopeVars, [targetKey(target)]: saved } }));
  return !!saved;
}

// ------------------------------------------------------------------------------- collection auth

/** Reads one collection's auth into the store. Quiet on failure, like `loadVariables`. */
export async function loadCollectionAuth(slug: string): Promise<CollectionAuthDto | undefined> {
  try {
    const dto = await invoke("collection_auth_get", { slug });
    setState((s) => ({ collectionAuth: { ...s.collectionAuth, [slug]: dto } }));
    return dto;
  } catch {
    return undefined;
  }
}

/** Replaces one collection's auth; a failure shows as a toast. `expectedRevision` guards against a stale overwrite. */
export async function saveCollectionAuth(slug: string, auth: Auth, expectedRevision: string | null): Promise<boolean> {
  const saved = await guarded(() => invoke("collection_auth_save", { input: { slug, auth, expectedRevision: expectedRevision ?? undefined } }));
  if (saved) setState((s) => ({ collectionAuth: { ...s.collectionAuth, [slug]: saved } }));
  return !!saved;
}

/** Sets one variable in a scope, on top of what is on disk now. */
export async function setVariable(target: VarTargetDto, name: string, value: string): Promise<boolean> {
  const current = await guarded(() => invoke("variables_get", { target }));
  if (!current) return false;
  return saveVariables(target, withVariable(current.variables, name, value), current.revision);
}

export const promptAddVariable = (name: string) => setState({ addingVariable: name });
export const dismissAddVariable = () => setState({ addingVariable: null });

// ----------------------------------------------------------------------------------------- requests

export async function openRequest(id: string) {
  const dto = await guarded(() => invoke("request_get", { id }));
  if (!dto) return;
  setState({ draft: draftFromDto(dto), route: "request" });
  void loadHistory(id);
}

/** Opens a spec operation as an unsaved draft. */
export async function openOperation(collection: string, spec: string, key: string) {
  const dto = await guarded(() => invoke("operation_open", { collection, spec, key }));
  if (dto) setState({ draft: draftFromDto(dto), route: "request" });
}

/** Leaves the request view. The caller asks first when `draft.dirty` (with the in-app dialog). */
export function closeRequest() {
  setState({ draft: null, route: "find" });
}

const updateDraft = (fn: (d: DraftState) => DraftState) =>
  setState((s) => (s.draft ? { draft: fn(s.draft) } : {}));

export const editDraft = (patch: Partial<RequestDoc>) => updateDraft((d) => applyEdit(d, patch));
export const setDraftLocation = (collection: string | null, folder: string[]) => updateDraft((d) => applyMove(d, collection, folder));

/** Saves the draft with `expectedRevision`. On `revision_conflict` the draft shows the conflict banner instead of failing. */
export async function saveRequest(opts: { overwrite?: boolean } = {}): Promise<boolean> {
  const d = getState().draft;
  if (!d || d.saving || d.base.readOnly) return false;
  const collection = d.collection ?? getState().tree?.collections[0]?.slug ?? null;
  if (!collection) {
    toast("Create a collection first, then save.", "error");
    return false;
  }
  const sentDoc = d.doc;
  updateDraft((x) => ({ ...x, saving: true, saveError: null }));
  try {
    const saved = await invoke("request_save", {
      doc: sentDoc,
      collection,
      folder: d.folder,
      expectedRevision: opts.overwrite || d.expectedRevision === null ? undefined : d.expectedRevision,
    });
    updateDraft((x) => draftSaved(x, saved, sentDoc));
    void refreshTree();
    return true;
  } catch (e) {
    const err = toHopperError(e);
    if (err.code === "revision_conflict") updateDraft((x) => ({ ...x, saving: false, conflict: true, saveError: null }));
    else updateDraft((x) => ({ ...x, saving: false, saveError: err }));
    return false;
  }
}

/** Conflict or external-change banner, "Reload": drop my edits and take what is on disk. */
export async function reloadDraft() {
  const id = getState().draft?.doc.id;
  if (!id) return;
  const dto = await guarded(() => invoke("request_get", { id }));
  if (dto) updateDraft((d) => ({ ...draftFromDto(dto), runId: d.runId }));
}

/** Banner, "Keep mine": adopt the disk revision as the base so the next save goes through, keeping my edits. */
export async function keepMine() {
  const id = getState().draft?.doc.id;
  if (!id) return;
  const dto = await guarded(() => invoke("request_get", { id }));
  if (!dto) return;
  updateDraft((d) => {
    const next = { ...d, base: dto, expectedRevision: dto.revision, conflict: false, external: false };
    return { ...next, dirty: true };
  });
}

export async function deleteRequest(id: string) {
  const res = await guarded(() => invoke("request_delete", { id }));
  if (!res) return;
  const name = getState().draft?.doc.id === id ? getState().draft?.doc.name : undefined;
  if (getState().draft?.doc.id === id) closeRequest();
  await refreshTree();
  toast(`Deleted ${name ?? "request"}`, "info", {
    undo: {
      label: "Undo",
      run: async () => {
        const dto = await guarded(() => invoke("request_undelete", { undoToken: res.undoToken }));
        if (dto) await refreshTree();
      },
    },
  });
}

export async function duplicateRequest(id: string) {
  const dto = await guarded(() => invoke("request_duplicate", { id }));
  if (dto) {
    await refreshTree();
    await openRequest(dto.doc.id);
  }
}

export async function moveRequest(id: string, collection: string, folder: string[]) {
  const dto = await guarded(() => invoke("request_move", { id, collection, folder }));
  if (dto) await refreshTree();
}

export async function setAgentAccess(requestIds: string[], access: "ask" | "auto" | "blocked") {
  await guarded(() => invoke("agent_access_set", { requestIds, access }));
  await refreshTree();
  const open = getState().draft?.doc.id;
  if (open && requestIds.includes(open) && !getState().draft?.dirty) await reloadDraft();
  else if (open && requestIds.includes(open)) {
    updateDraft((d) => ({ ...d, base: { ...d.base, agentAccess: access, grantSuspended: false } }));
  }
}

// ----------------------------------------------------------------------------------------- sending

let runSeq = 0;
const newRunId = () => `run-${Date.now().toString(36)}-${++runSeq}`;

/** Sends the open draft. Returns the result, or `null` when it failed or was cancelled (the run state holds the error). */
export async function send(overrides?: Record<string, string>): Promise<RunResult | null> {
  const d = getState().draft;
  if (!d) return null;
  const runId = newRunId();
  const saved = d.base.revision !== null && !!d.doc.id;
  const target = saved && !d.dirty ? { requestId: d.doc.id } : { draft: d.doc, collection: d.collection, folder: d.folder };
  setState((s) => ({
    runs: pruneRuns({ ...s.runs, [runId]: newRun(runId, saved ? d.doc.id : null, Date.now()) }),
    draft: s.draft ? { ...s.draft, runId } : s.draft,
  }));
  const attempt = (confirmedHost?: string) => invoke("request_send", { runId, target, overrides, confirmedHost });
  try {
    let result: RunResult;
    try {
      result = await attempt();
    } catch (e) {
      const err = toHopperError(e);
      if (err.code !== "confirm_unlisted_host") throw e;
      const host = String(err.details?.host ?? "");
      const ok = await confirm({
        title: "Send secrets to an unlisted host?",
        message: `This environment's secrets would be sent to ${host}, which is not in its allowed hosts.`,
        confirmLabel: "Send anyway",
        tone: "danger",
      });
      if (!ok) {
        setState((s) => ({ runs: { ...s.runs, [runId]: { ...(s.runs[runId] ?? newRun(runId, null, Date.now())), phase: "cancelled", error: null } } }));
        return null;
      }
      result = await attempt(host);
    }
    setState((s) => ({ runs: { ...s.runs, [runId]: runFinished(s.runs[runId] ?? newRun(runId, null, Date.now()), result) } }));
    void refreshRecent();
    if (result.requestId) void loadHistory(result.requestId);
    return result;
  } catch (e) {
    const err = toHopperError(e);
    setState((s) => ({ runs: { ...s.runs, [runId]: runFailed(s.runs[runId] ?? newRun(runId, null, Date.now()), err) } }));
    return null;
  }
}

export async function cancel(runId?: string) {
  const id = runId ?? getState().draft?.runId;
  if (!id) return;
  await guarded(() => invoke("request_cancel", { runId: id }));
}

export async function loadHistory(requestId: string) {
  const items = await guarded(() => invoke("history_list", { requestId }));
  if (items) setState((s) => ({ history: { ...s.history, [requestId]: items } }));
}

/** Shows an older run in the response viewer. */
export async function openHistoryRun(requestId: string, id: string): Promise<RunResult | undefined> {
  const run = await guarded(() => invoke("history_get", { requestId, id }));
  if (!run) return undefined;
  setState((s) => ({
    runs: { ...s.runs, [run.runId]: runFinished(newRun(run.runId, requestId, Date.now()), run) },
    draft: s.draft && s.draft.doc.id === requestId ? { ...s.draft, runId: run.runId } : s.draft,
  }));
  return run;
}

export async function copyCurl(masked = true): Promise<string | undefined> {
  const d = getState().draft;
  if (!d) return undefined;
  const saved = d.base.revision !== null && !!d.doc.id && !d.dirty;
  return guarded(() =>
    invoke("copy_curl", { masked, target: saved ? { requestId: d.doc.id } : { draft: d.doc, collection: d.collection, folder: d.folder } }),
  );
}

// ------------------------------------------------------------------------------------------ search

const searchTicket = latestWins();
const searchTimer = debounced(() => void runSearch(), SEARCH_DEBOUNCE_MS);

/** Updates the search inputs now and queries the backend after a short pause; only the newest answer is kept. */
export function search(patch: Partial<Pick<ReturnType<typeof getState>["search"], "query" | "tag" | "method">>) {
  setState((s) => ({ search: { ...s.search, ...patch } }));
  const { query, tag, method } = getState().search;
  if (!query.trim() && !tag && !method) {
    searchTimer.cancel();
    searchTicket.invalidate();
    setState((s) => ({ search: { ...s.search, loading: false, result: null, error: null } }));
    return;
  }
  setState((s) => ({ search: { ...s.search, loading: true } }));
  searchTimer.schedule();
}

export const setSearchTag = (tag: string | null) => search({ tag });
export const setSearchMethod = (method: Method | null) => search({ method });

export function clearSearch() {
  search({ query: "", tag: null, method: null });
}

/** Runs the search immediately (also what the debounce calls). */
export async function runSearch() {
  searchTimer.cancel();
  const { query, tag, method } = getState().search;
  const ticket = searchTicket.begin();
  try {
    const result = await invoke("search_routes", { query, tag: tag ?? undefined, method: method ?? undefined, limit: 50 });
    if (ticket.current()) setState((s) => ({ search: { ...s.search, loading: false, result, error: null } }));
  } catch (e) {
    if (ticket.current()) setState((s) => ({ search: { ...s.search, loading: false, error: toHopperError(e) } }));
  }
}

// ------------------------------------------------------------------------------------- agent, mcp

export async function refreshMcp() {
  const mcp = await invoke("mcp_status").catch(() => null);
  if (mcp) setState({ mcp });
}

export async function respondApproval(id: string, decision: ApprovalDecision) {
  const ok = await guarded(() => invoke("approval_respond", { id, decision }));
  if (ok !== undefined) setState((s) => ({ approvals: s.approvals.filter((a) => a.id !== id) }));
  if (decision === "always_allow" || decision === "block") void refreshTree();
}

// ------------------------------------------------------------------------------------------ settings

export async function updateSettings(patch: Partial<Settings>) {
  const current = getState().settings;
  if (!current) return;
  const next = { ...current, ...patch };
  setState({ settings: next });
  const saved = await guarded(() => invoke("settings_set", { settings: next }));
  if (saved) setState({ settings: saved });
  else setState({ settings: current });
}

// -------------------------------------------------------------------------- events and first load

/** The user looked at the floating Hopper's notices (opened the panel or clicked one). */
export function markNoticesSeen() {
  if (getState().notices.some((n) => !n.seen)) setState((s) => ({ notices: s.notices.map((n) => (n.seen ? n : { ...n, seen: true })) }));
}

/** Re-reads the open request after a disk change: a clean draft reloads, a dirty one gets the banner. */
async function reconcileDraftWithDisk() {
  const d = getState().draft;
  if (!d || !d.doc.id || d.base.revision === null || d.saving) return;
  let disk = null;
  try {
    disk = await invoke("request_get", { id: d.doc.id });
  } catch (e) {
    if (!isHopperError(e, "not_found")) return;
  }
  updateDraft((cur) => (cur.doc.id === d.doc.id ? draftMeetsDisk(cur, disk) : cur));
}

let disposeEvents: (() => void) | null = null;

/** Loads the first state and subscribes to every backend event, once. Returns the dispose function. Calling it again resets. */
export function initStore(): () => void {
  disposeEvents?.();
  const offs = [
    listen("workspace://changed", () => {
      void refreshTree();
      void refreshRecent();
      void refreshEnvironments();
      void refreshScopeVars();
      void reconcileDraftWithDisk();
    }),
    listen("run://progress", ({ runId, phase }) =>
      setState((s) => (s.runs[runId] ? { runs: { ...s.runs, [runId]: runProgress(s.runs[runId], phase) } } : {})),
    ),
    listen("secrets://status", () => void refreshEnvironments()),
    listen("agent://activity", (entry) => {
      setState((s) => ({ activity: s.activity.some((a) => a.id === entry.id) ? s.activity : [entry, ...s.activity].slice(0, ACTIVITY_CAP) }));
      void refreshMcp();
    }),
    listen("agent://endpoints-added", (added) => {
      setState((s) => ({
        notices: [{ ...added, id: `notice-${Date.now()}-${s.notices.length}`, at: new Date().toISOString(), seen: false }, ...s.notices].slice(0, NOTICE_CAP),
      }));
      // The open panel has no bubble: the same news arrives as a toast that opens the request.
      const requestId = added.requestId;
      toast(`${added.client} added ${added.label}`, "info", requestId ? { undo: { label: "Open", run: () => void openRequest(requestId) } } : {});
    }),
    listen("agent://approval-request", (a) =>
      setState((s) => ({ approvals: s.approvals.some((x) => x.id === a.id) ? s.approvals : [...s.approvals, a] })),
    ),
    listen("agent://approval-resolved", ({ id }) => setState((s) => ({ approvals: s.approvals.filter((a) => a.id !== id) }))),
    listen("panel://show-request", ({ id }) => void openRequest(id)),
    listen("panel://open-sheet", ({ sheet }) => openSheet(sheet)),
    listen("mcp://status", (mcp) => setState({ mcp })),
    listen("update://check", () => void checkForUpdates()),
  ];
  let alive = true;
  void (async () => {
    try {
      // Settings do not depend on a workspace; activity and approvals do, and reject with no_workspace without one.
      const settings = await invoke("settings_get");
      if (!alive) return;
      setState({ settings });
      const [activity, approvals] = await Promise.all([
        invoke("agent_activity_list").catch((e) => (isHopperError(e, "no_workspace") ? [] : Promise.reject(e))),
        invoke("approval_pending").catch((e) => (isHopperError(e, "no_workspace") ? [] : Promise.reject(e))),
      ]);
      if (!alive) return;
      setState({ activity: activity.slice(0, ACTIVITY_CAP), approvals });
      await Promise.all([loadWorkspace(), refreshMcp()]);
    } catch (e) {
      setState({ error: toHopperError(e) as HopperError });
    } finally {
      if (alive) setState({ ready: true });
    }
  })();
  disposeEvents = () => {
    alive = false;
    offs.forEach((off) => off());
    disposeEvents = null;
  };
  return disposeEvents;
}

/** Back to the empty state and no pending timers. Tests call this between cases. */
export function resetStore() {
  disposeEvents?.();
  searchTimer.cancel();
  searchTicket.invalidate();
  store.replaceState(initialState());
}
