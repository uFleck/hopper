import type { AgentAccess, Method, TreeDto, TreeNode } from "../../ipc";

export interface Segment {
  text: string;
  hit: boolean;
}

/** Splits `text` into runs of matched and unmatched characters; `matches` are character indices, in any order. */
export function highlightSegments(text: string, matches: number[]): Segment[] {
  if (!matches.length) return [{ text, hit: false }];
  const marked = new Set(matches);
  const out: Segment[] = [];
  for (let i = 0; i < text.length; i++) {
    const hit = marked.has(i);
    const last = out[out.length - 1];
    if (last && last.hit === hit) last.text += text[i];
    else out.push({ text: text[i], hit });
  }
  return out;
}

export interface FlatRequest {
  id: string;
  name: string;
  method: Method;
  agentAccess: AgentAccess;
  grantSuspended: boolean;
  orphaned: boolean;
  collection: string;
  folder: string[];
}

/** Every saved request in the tree with where it lives. */
export function flattenRequests(tree: TreeDto | null): FlatRequest[] {
  const out: FlatRequest[] = [];
  const walk = (nodes: TreeNode[], collection: string, folder: string[]) => {
    for (const n of nodes) {
      if (n.type === "folder") walk(n.children, collection, [...folder, n.slug]);
      else out.push({ id: n.id, name: n.name, method: n.method, agentAccess: n.agentAccess, grantSuspended: n.grantSuspended, orphaned: n.orphaned, collection, folder });
    }
  };
  for (const c of tree?.collections ?? []) walk(c.children, c.slug, []);
  return out;
}

export interface FolderOption {
  collection: string;
  folder: string[];
  label: string;
}

/** Every place a request can be moved to: each collection root and each folder, labelled `Collection / Folder / Sub`. */
export function folderOptions(tree: TreeDto | null): FolderOption[] {
  const out: FolderOption[] = [];
  const walk = (nodes: TreeNode[], collection: string, folder: string[], names: string[]) => {
    for (const n of nodes) {
      if (n.type !== "folder") continue;
      const path = [...folder, n.slug];
      const label = [...names, n.name];
      out.push({ collection, folder: path, label: label.join(" / ") });
      walk(n.children, collection, path, label);
    }
  };
  for (const c of tree?.collections ?? []) {
    out.push({ collection: c.slug, folder: [], label: c.name });
    walk(c.children, c.slug, [], [c.name]);
  }
  return out;
}

/** Ids of the requests under `nodes` (folders included, recursively) that pass `keep`. */
export function requestIdsIn(nodes: TreeNode[], keep: (r: Extract<TreeNode, { type: "request" }>) => boolean = () => true): string[] {
  const out: string[] = [];
  for (const n of nodes) {
    if (n.type === "folder") out.push(...requestIdsIn(n.children, keep));
    else if (keep(n)) out.push(n.id);
  }
  return out;
}

export const ACCESS_LABEL: Record<AgentAccess, string> = {
  ask: "Ask",
  auto: "Auto",
  auto_params: "Auto: parameters may change",
  auto_trusted: "Auto: trusted, any edit",
  blocked: "Blocked",
};

/** Compact labels for chips in lists. */
export const ACCESS_SHORT: Record<AgentAccess, string> = { ...ACCESS_LABEL, auto_params: "Auto · params", auto_trusted: "Auto · trusted" };

/** The prompt-injection risk of the two loose Auto modes, in plain words. Shown wherever one is chosen. */
export const LOOSE_AUTO_RISK: Partial<Record<AgentAccess, string>> = {
  auto_params:
    "The agent can send this request with any path and query values, without asking. Method, URL path, headers, body and auth stay as you saved them. If a page, file or message the agent reads tricks it (a prompt injection), it could use other values than you meant, such as another user's id.",
  auto_trusted:
    "The agent can change anything in this request (method, path, headers, body) and send it without asking. Only the host, environment and inherited auth stay fixed. If a page, file or message the agent reads tricks it (a prompt injection), it could turn this into a different call to the same API.",
};

export const optionKey = (o: { collection: string; folder: string[] }) => [o.collection, ...o.folder].join("/");

/** Splits a typed or pasted path into its last segment, for a default workspace name. */
export function nameFromPath(path: string): string {
  return path.trim().replace(/[\\/]+$/, "").split(/[\\/]/).pop() ?? "";
}
