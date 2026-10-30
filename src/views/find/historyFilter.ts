import type { HistoryRow } from "../../ipc";

type Searchable = Pick<HistoryRow, "label" | "name" | "url" | "method" | "status">;

/** The lower-cased words of a search box, empty words dropped. */
export function queryWords(query: string): string[] {
  return query.toLowerCase().split(/\s+/).filter(Boolean);
}

/** True when every word of `query` appears in the label, request name, URL, method or status (case-insensitive). */
export function matchesHistory(row: Searchable, query: string): boolean {
  const words = queryWords(query);
  const fields = [row.label ?? "", row.name, row.url, row.method, String(row.status)].map((f) => f.toLowerCase());
  return words.every((w) => fields.some((f) => f.includes(w)));
}

/** The `[start, end)` ranges of every query word inside `text`, overlaps merged, sorted; for `<mark>`. */
export function highlightRanges(text: string, query: string): [number, number][] {
  const lower = text.toLowerCase();
  const found: [number, number][] = [];
  for (const w of queryWords(query)) {
    let at = lower.indexOf(w);
    while (at !== -1) {
      found.push([at, at + w.length]);
      at = lower.indexOf(w, at + w.length);
    }
  }
  found.sort((a, b) => a[0] - b[0]);
  const merged: [number, number][] = [];
  for (const r of found) {
    const last = merged[merged.length - 1];
    if (last && r[0] <= last[1]) last[1] = Math.max(last[1], r[1]);
    else merged.push([r[0], r[1]]);
  }
  return merged;
}
