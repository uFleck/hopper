/* Route search for the mock: AND of fuzzy tokens, method prefix, `tag:`, `#opId`, `path:`. A demo-grade take on PLAN section 3. */
import type { Method, RequestDto, SearchHit, SearchQuery, SearchResult } from "../../types";
import { COLLECTION, SPEC, type MockOperation } from "./seed";

const METHODS: Method[] = ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];

export interface ParsedQuery {
  words: string[];
  method?: Method;
  tag?: string;
  opId?: string;
  path?: string;
}

export function parseQuery(query: string): ParsedQuery {
  const out: ParsedQuery = { words: [] };
  const tokens = query.trim().split(/\s+/).filter(Boolean);
  tokens.forEach((t, i) => {
    const lower = t.toLowerCase();
    if (lower.startsWith("tag:") && t.length > 4) out.tag = lower.slice(4);
    else if (lower.startsWith("path:") && t.length > 5) out.path = lower.slice(5);
    else if (t.startsWith("#") && t.length > 1) out.opId = lower.slice(1);
    else if (i === 0 && METHODS.includes(t.toUpperCase() as Method) && (tokens.length > 1 || t.length >= 3)) out.method = t.toUpperCase() as Method;
    else out.words.push(lower);
  });
  return out;
}

/** Fuzzy match of `token` in `text`: a contiguous run when there is one, else a subsequence. Returns indices and a score. */
export function fuzzy(text: string, token: string): { indices: number[]; score: number } | null {
  const lower = text.toLowerCase();
  const at = lower.indexOf(token);
  if (at >= 0) {
    const boundary = at === 0 || /[/\-_.{]/.test(lower[at - 1]);
    const endsSegment = at + token.length === lower.length || /[/\-_.}]/.test(lower[at + token.length]);
    const score = boundary && endsSegment ? 100 : boundary ? 60 : 30;
    return { indices: Array.from({ length: token.length }, (_, k) => at + k), score };
  }
  const indices: number[] = [];
  let from = 0;
  for (const ch of token) {
    const i = lower.indexOf(ch, from);
    if (i < 0) return null;
    indices.push(i);
    from = i + 1;
  }
  return { indices, score: 10 };
}

interface Candidate {
  hit: SearchHit;
  method: Method;
  tags: string[];
  opId: string;
  path: string;
}

function evaluate(c: { path: string; haystack: string[] }, words: string[]): { matches: number[]; score: number } | null {
  let score = 0;
  const matches = new Set<number>();
  for (const w of words) {
    const onPath = fuzzy(c.path, w);
    if (onPath) {
      score += onPath.score + 5;
      onPath.indices.forEach((i) => matches.add(i));
      continue;
    }
    let best = 0;
    for (const h of c.haystack) best = Math.max(best, fuzzy(h, w)?.score ?? 0);
    if (!best) return null;
    score += best;
  }
  return { matches: [...matches].sort((a, b) => a - b), score };
}

const pathOf = (url: string) => {
  const noHost = url.replace(/^\{\{[^}]+\}\}/, "").replace(/^https?:\/\/[^/]+/, "");
  return noHost.split("?")[0] || "/";
};

export function searchRoutes(q: SearchQuery, operations: MockOperation[], saved: RequestDto[]): SearchResult {
  const p = parseQuery(q.query);
  const candidates: Candidate[] = [];

  for (const r of saved) {
    if (q.collection && r.collection !== q.collection) continue;
    const path = pathOf(r.doc.url);
    const key = r.doc.source?.key ?? "";
    const ev = evaluate({ path, haystack: [r.doc.name, key] }, p.words);
    if (!ev) continue;
    candidates.push({
      method: r.doc.method,
      tags: [],
      opId: key.toLowerCase(),
      path,
      hit: {
        kind: "saved",
        requestId: r.doc.id,
        name: r.doc.name,
        method: r.doc.method,
        path,
        collection: r.collection ?? COLLECTION,
        matches: ev.matches,
        score: ev.score + 20,
        agentAccess: r.agentAccess,
      },
    });
  }

  for (const op of operations) {
    if (q.collection && q.collection !== COLLECTION) continue;
    const ev = evaluate({ path: op.path, haystack: [op.key, op.tag, op.summary] }, p.words);
    if (!ev) continue;
    const savedReq = saved.find((r) => r.doc.source?.spec === SPEC && r.doc.source.key === op.key && !r.doc.source.orphaned);
    candidates.push({
      method: op.method,
      tags: [op.tag],
      opId: op.key.toLowerCase(),
      path: op.path,
      hit: {
        kind: "operation",
        collection: COLLECTION,
        spec: SPEC,
        key: op.key,
        method: op.method,
        path: op.path,
        summary: op.summary,
        tags: [op.tag],
        deprecated: !!op.deprecated,
        matches: ev.matches,
        score: ev.score - (op.deprecated ? 50 : 0),
        savedRequestId: savedReq?.doc.id,
      },
    });
  }

  // Syntax filters narrow the text matches; they define the match set the facets count.
  const matched = candidates.filter((c) => {
    if (p.method && c.method !== p.method) return false;
    if (p.tag && !(c.tags.some((t) => t.toLowerCase().includes(p.tag!)) )) return false;
    if (p.opId && !fuzzy(c.opId, p.opId)) return false;
    if (p.path && !fuzzy(c.path, p.path)) return false;
    return true;
  });

  const tagCounts = new Map<string, number>();
  const methodCounts = new Map<Method, number>();
  for (const c of matched) {
    c.tags.forEach((t) => tagCounts.set(t, (tagCounts.get(t) ?? 0) + 1));
    methodCounts.set(c.method, (methodCounts.get(c.method) ?? 0) + 1);
  }

  const narrowed = matched.filter((c) => (!q.method || c.method === q.method) && (!q.tag || c.tags.includes(q.tag)));
  narrowed.sort((a, b) => b.hit.score - a.hit.score || a.path.localeCompare(b.path));

  return {
    hits: narrowed.slice(0, q.limit ?? 50).map((c) => c.hit),
    total: narrowed.length,
    tags: [...tagCounts].map(([name, count]) => ({ name, count })).sort((a, b) => b.count - a.count || a.name.localeCompare(b.name)),
    methods: [...methodCounts].map(([method, count]) => ({ method, count })).sort((a, b) => b.count - a.count),
  };
}
