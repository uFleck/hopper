import type {
  ActivityEntry,
  ApprovalRequest,
  EndpointsAdded,
  Stage,
  StageDto,
  HistoryItem,
  HopperError,
  McpStatus,
  Method,
  RecentItem,
  RecentWorkspace,
  SearchResult,
  Settings,
  TreeDto,
  CollectionAuthDto,
  VariablesDto,
  WorkspaceInfo,
} from "../ipc";
import { createStore, useStoreSelector } from "./core";
import type { AgentTab, DraftState, Route, RunState, SheetName } from "./reducers";

export interface SearchState {
  query: string;
  tag: string | null;
  method: Method | null;
  loading: boolean;
  result: SearchResult | null;
  error: HopperError | null;
}

export type ToastKind = "info" | "success" | "error";

export interface Toast {
  id: string;
  kind: ToastKind;
  message: string;
  /** Present on an undo toast: the button label and what it does. */
  undo?: { label: string; run: () => void | Promise<void> };
  /** Milliseconds until it goes away; `0` keeps it until dismissed. */
  ttl: number;
}

/** An endpoint an agent added, announced by the floating Hopper until the user has seen it. */
export interface EndpointNotice extends EndpointsAdded {
  id: string;
  at: string;
  seen: boolean;
}

export interface AppState {
  /** The first load (workspace, settings, environments) finished. */
  ready: boolean;
  workspace: WorkspaceInfo | null;
  recentWorkspaces: RecentWorkspace[];
  tree: TreeDto | null;
  recent: RecentItem[];
  environments: StageDto[];
  /** Variables of the scopes the open request resolves against, by `targetKey`; plus any the sheets loaded. */
  scopeVars: Record<string, VariablesDto>;
  /** Collection auth by slug, as the service last returned it. */
  collectionAuth: Record<string, CollectionAuthDto>;
  /** A variable name the user asked to define from a hover: the in-app "add variable" dialog shows while set. */
  addingVariable: string | null;
  settings: Settings | null;
  route: Route;
  sheet: SheetName | null;
  /** Tab the agent sheet opens on; `null` means its default. */
  agentTab: AgentTab | null;
  draft: DraftState | null;
  runs: Record<string, RunState>;
  history: Record<string, HistoryItem[]>;
  search: SearchState;
  approvals: ApprovalRequest[];
  activity: ActivityEntry[];
  notices: EndpointNotice[];
  mcp: McpStatus | null;
  toasts: Toast[];
  /** A command failed and nobody handled it: an in-app dialog shows it. */
  error: HopperError | null;
}

export const initialSearch: SearchState = { query: "", tag: null, method: null, loading: false, result: null, error: null };

export const initialState = (): AppState => ({
  ready: false,
  workspace: null,
  recentWorkspaces: [],
  tree: null,
  recent: [],
  environments: [],
  scopeVars: {},
  collectionAuth: {},
  addingVariable: null,
  settings: null,
  route: "find",
  sheet: null,
  agentTab: null,
  draft: null,
  runs: {},
  history: {},
  search: initialSearch,
  approvals: [],
  activity: [],
  notices: [],
  mcp: null,
  toasts: [],
  error: null,
});

export const store = createStore<AppState>(initialState());

/** `const name = useStore((s) => s.workspace?.name)`. The selector must return a stable value. */
export const useStore = <S,>(selector: (s: AppState) => S): S => useStoreSelector(store, selector);

// ------------------------------------------------------------------------------------------ selectors

/** The active stage, or null when none is active. */
export const selectActiveEnv = (s: AppState): Stage | null => s.environments.find((e) => e.active)?.stage ?? null;

export const selectPendingRun = (s: AppState): RunState | null =>
  Object.values(s.runs).find((r) => r.phase === "preparing" || r.phase === "sending" || r.phase === "receiving") ?? null;

export const selectDraftRun = (s: AppState): RunState | null => (s.draft?.runId ? (s.runs[s.draft.runId] ?? null) : null);

export type MascotKind = "sleeping" | "sending" | "agent" | "approval" | "error";

/** What the mascot shows: approvals first, then a send in flight, recent agent activity, a failed run, else asleep. */
export function mascotKind(s: AppState, now = Date.now()): MascotKind {
  if (s.approvals.length) return "approval";
  if (selectPendingRun(s)) return "sending";
  const last = s.activity[0];
  if (last && now - Date.parse(last.at) < 4000) return "agent";
  const run = selectDraftRun(s);
  if (run?.phase === "failed" || (run?.result && run.result.status >= 500)) return "error";
  return "sleeping";
}
