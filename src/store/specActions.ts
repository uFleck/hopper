/* Spec import and the "show more" page of the search, kept out of actions.ts (owned by the store). */
import { invoke, toHopperError, type SpecImportResult, type SpecSource } from "../ipc";
import { refreshTree, toast } from "./actions";
import { store } from "./state";

const { getState, setState } = store;

/** Imports a spec into `collection`, creating the collection first when it does not exist. Resolves with the result or `undefined` (an error toast was shown). */
export async function importSpec(collection: string | { create: string }, source: SpecSource, name?: string): Promise<SpecImportResult | undefined> {
  try {
    const slug = typeof collection === "string" ? collection : await invoke("collection_create", { name: collection.create });
    const result = await invoke("spec_import", { collection: slug, source, name: name || undefined });
    await refreshTree();
    toast(`Imported ${result.title}: ${result.operationCount} operations`, "success");
    return result;
  } catch (e) {
    toast(toHopperError(e).message, "error");
    return undefined;
  }
}

export const SEARCH_PAGE = 50;
export const SEARCH_MAX = 300;

/** Asks the backend for the next page of the current search (the store's own search asks for the first 50). */
export async function searchMore(): Promise<void> {
  const { query, tag, method, result } = getState().search;
  if (!result || result.hits.length >= Math.min(result.total, SEARCH_MAX)) return;
  const limit = Math.min(result.hits.length + SEARCH_PAGE, SEARCH_MAX);
  try {
    const next = await invoke("search_routes", { query, tag: tag ?? undefined, method: method ?? undefined, limit });
    const now = getState().search;
    if (now.query === query && now.tag === tag && now.method === method) setState((s) => ({ search: { ...s.search, result: next } }));
  } catch (e) {
    toast(toHopperError(e).message, "error");
  }
}
