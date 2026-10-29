import { useSyncExternalStore } from "react";
import type { TreeDto, TreeNode } from "../ipc";

const KEY = "hopper.tree.open";
const listeners = new Set<() => void>();

const read = (): ReadonlySet<string> => {
  try {
    const v: unknown = JSON.parse(localStorage.getItem(KEY) ?? "[]");
    return new Set(Array.isArray(v) ? v.filter((x): x is string => typeof x === "string") : []);
  } catch {
    return new Set();
  }
};

let open = read();

function write(next: ReadonlySet<string>) {
  open = next;
  try {
    localStorage.setItem(KEY, JSON.stringify([...next]));
  } catch {
    /* storage unavailable: the open nodes last for this session */
  }
  listeners.forEach((l) => l());
}

const subscribe = (l: () => void) => {
  listeners.add(l);
  return () => void listeners.delete(l);
};

/** The collections and folders the user expanded on the home screen. Kept across restarts and workspaces: a key never matches a node in another workspace unless it has the same slugs. */
export const getOpenNodes = () => open;
export const useOpenNodes = () => useSyncExternalStore(subscribe, getOpenNodes);

export const collectionKey = (collection: string) => `c:${collection}`;
export const folderKey = (collection: string, path: string[]) => `f:${collection}/${path.join("/")}`;

export const toggleNode = (key: string) => {
  const next = new Set(open);
  if (!next.delete(key)) next.add(key);
  write(next);
};

/** Every collection and folder key in the tree. */
export function nodeKeys(tree: TreeDto | null): string[] {
  const out: string[] = [];
  const walk = (nodes: TreeNode[], collection: string, path: string[]) => {
    for (const n of nodes) {
      if (n.type !== "folder") continue;
      const next = [...path, n.slug];
      out.push(folderKey(collection, next));
      walk(n.children, collection, next);
    }
  };
  for (const c of tree?.collections ?? []) {
    out.push(collectionKey(c.slug));
    walk(c.children, c.slug, []);
  }
  return out;
}

/** Expands or collapses the nodes of this tree only; other workspaces keep their state. */
export function setNodes(keys: string[], expanded: boolean) {
  const next = new Set(open);
  for (const k of keys) {
    if (expanded) next.add(k);
    else next.delete(k);
  }
  write(next);
}

/** Test hook: forget everything in memory and storage. */
export const resetTreeOpen = () => write(new Set());
