import type { ApplicationEnvDto, Auth, Body, KeyValue, NameValue, RequestDoc, Stage, StageDto, TreeNode, VarTargetDto } from "../../ipc";
import { NO_LAYERS, type ScopeLayers } from "../../store/scopes";

/** The variables a request resolves against: the active stage's section for the request's collection (all sections when none matches). */
export type ResolvedEnv = { name: Stage; readOnly: boolean; applicationOf: Map<string, string> } & Pick<ApplicationEnvDto, "variables" | "secrets" | "tls">;

export function resolveEnv(stage: StageDto | undefined, collection: string | null | undefined): ResolvedEnv | null {
  if (!stage) return null;
  const match = stage.applications.filter((a) => a.application === collection);
  const apps = match.length ? match : stage.applications;
  return {
    name: stage.stage,
    readOnly: stage.readOnly,
    applicationOf: new Map(apps.flatMap((a) => a.variables.map((v) => [v.name, a.application] as const))),
    variables: apps.flatMap((a) => a.variables),
    secrets: apps.flatMap((a) => a.secrets),
    tls: { insecureSkipVerify: apps.some((a) => a.tls.insecureSkipVerify) },
  };
}

export const MASK = "••••••••";

const VAR_RE = /\{\{\s*([^{}\s]+)\s*\}\}/g;
const PATH_PARAM_RE = /(?<!\{)\{([A-Za-z_][\w.-]*)\}(?!\})/g;

export const isBuiltin = (name: string) => name.startsWith("$");

/** Names inside `{{ }}` in the text, in order of first appearance. */
export function variablesIn(text: string): string[] {
  return [...new Set([...text.matchAll(VAR_RE)].map((m) => m[1]))];
}

/** The name of the `{{ name }}` token covering character `index`, or null. */
export function variableAt(text: string, index: number): string | null {
  for (const m of text.matchAll(VAR_RE)) {
    const at = m.index ?? 0;
    if (index >= at && index < at + m[0].length) return m[1];
  }
  return null;
}

/** The name inside one `{{ name }}` token. */
export const variableName = (token: string) => token.replace(/^\{\{\s*|\s*\}\}$/g, "");

/** `{id}` placeholders of a URL template; `{{vars}}` are not path params. */
export function pathParamNames(url: string): string[] {
  return [...new Set([...url.matchAll(PATH_PARAM_RE)].map((m) => m[1]))];
}

export type VarScopeKind = "request" | "folder" | "collection" | "environment" | "workspace";

/** Where a variable's winning value is stored, and what a save has to write to. */
export type VarEdit = { type: "scope"; target: VarTargetDto } | { type: "environment"; stage: Stage; application: string };

export interface VarSource {
  kind: VarScopeKind;
  /** Which one: the collection slug, the folder path or the stage; empty for the request and the workspace. */
  detail: string;
  /** Null when the value cannot be changed here, e.g. a read-only environment. */
  edit: VarEdit | null;
}

export interface VarInfo {
  /** Plain variables and secrets that resolve, by name. */
  known: Set<string>;
  values: Map<string, string>;
  secrets: Map<string, "set" | "missing">;
  /** Where each plain variable's winning value comes from. */
  origins: Map<string, string>;
  /** The same as `origins`, typed, with the target an edit writes to. */
  sources: Map<string, VarSource>;
}

const targetDetail = (t: VarTargetDto) => (t.type === "collection" ? t.slug : t.type === "folder" ? t.path.join("/") : "");

/** Precedence: request > folder > collection > environment > workspace defaults. */
export function varInfo(env: ResolvedEnv | null, layers: ScopeLayers = NO_LAYERS): VarInfo {
  const values = new Map<string, string>();
  const origins = new Map<string, string>();
  const sources = new Map<string, VarSource>();
  const secrets = new Map<string, "set" | "missing">();
  const apply = (origin: string, vars: NameValue[], source: (name: string) => VarSource) =>
    vars.forEach((v) => {
      values.set(v.name, v.value);
      origins.set(v.name, origin);
      sources.set(v.name, source(v.name));
    });
  const fromLayer = (l: ScopeLayers["below"][number]) => (): VarSource => ({ kind: l.target.type, detail: targetDetail(l.target), edit: { type: "scope", target: l.target } });
  layers.below.forEach((l) => apply(l.origin, l.vars, fromLayer(l)));
  if (env) {
    apply(`${env.name} environment`, env.variables, (name) => {
      const application = env.applicationOf.get(name);
      return { kind: "environment", detail: env.name, edit: application && !env.readOnly ? { type: "environment", stage: env.name, application } : null };
    });
  }
  layers.above.forEach((l) => apply(l.origin, l.vars, fromLayer(l)));
  env?.secrets.forEach((s) => secrets.set(s.name, s.status));
  return { known: new Set([...values.keys(), ...secrets.keys()]), values, secrets, origins, sources };
}

export const unresolvedNames = (text: string, info: VarInfo): string[] =>
  variablesIn(text).filter((n) => !isBuiltin(n) && !info.known.has(n));

export type SegmentKind = "text" | "var" | "secret" | "builtin" | "unresolved" | "missing";

export interface Segment {
  text: string;
  kind: SegmentKind;
}

function expand(template: string, info: VarInfo): Segment[] {
  const out: Segment[] = [];
  let last = 0;
  for (const m of template.matchAll(VAR_RE)) {
    const at = m.index ?? 0;
    if (at > last) out.push({ text: template.slice(last, at), kind: "text" });
    const name = m[1];
    if (isBuiltin(name)) out.push({ text: m[0], kind: "builtin" });
    else if (info.secrets.has(name)) out.push(info.secrets.get(name) === "set" ? { text: MASK, kind: "secret" } : { text: m[0], kind: "missing" });
    else if (info.values.has(name)) out.push({ text: info.values.get(name) ?? "", kind: "var" });
    else out.push({ text: m[0], kind: "unresolved" });
    last = at + m[0].length;
  }
  if (last < template.length) out.push({ text: template.slice(last), kind: "text" });
  return out;
}

export interface ResolvedUrl {
  segments: Segment[];
  text: string;
  /** `{param}` placeholders with no value. */
  emptyPathParams: string[];
}

/** The URL as it will be sent: variables replaced, secrets masked, path params filled, enabled query rows appended. */
export function resolveUrl(doc: RequestDoc, info: VarInfo): ResolvedUrl {
  const empty: string[] = [];
  const withPath = doc.url.replace(PATH_PARAM_RE, (m, n: string) => {
    const v = doc.pathParams[n];
    if (v) return v;
    empty.push(n);
    return m;
  });
  const segments = expand(withPath, info);
  const pairs: { name: string; value: Segment[] }[] = doc.query.filter((q) => q.enabled && q.name).map((q) => ({ name: q.name, value: expand(q.value, info) }));
  if (doc.auth.type === "apiKey" && doc.auth.in === "query" && doc.auth.name) pairs.push({ name: doc.auth.name, value: [{ text: MASK, kind: "secret" }] });
  let sep = withPath.includes("?") ? "&" : "?";
  for (const p of pairs) {
    segments.push({ text: `${sep}${p.name}=`, kind: "text" }, ...p.value);
    sep = "&";
  }
  return { segments, text: segments.map((s) => s.text).join(""), emptyPathParams: [...new Set(empty)] };
}

const kvText = (rows: KeyValue[]) => rows.filter((r) => r.enabled).flatMap((r) => [r.name, r.value]);

function authText(a: Auth): string[] {
  switch (a.type) {
    case "bearer": return [a.token];
    case "basic": return [a.username, a.password];
    case "apiKey": return [a.name, a.value];
    default: return [];
  }
}

function bodyText(b: Body): string[] {
  switch (b.type) {
    case "json": case "text": case "xml": return [b.content];
    case "form": return kvText(b.fields);
    case "multipart": return b.parts.filter((p) => p.enabled).flatMap((p) => ["value" in p ? p.value : p.file, p.name]);
    case "binary": return [b.file];
    default: return [];
  }
}

/** Every variable the request uses that the active environment does not define, across URL, params, headers, body and auth. */
export function allUnresolved(doc: RequestDoc, info: VarInfo): string[] {
  const text = [doc.url, ...Object.values(doc.pathParams), ...kvText(doc.query), ...kvText(doc.headers), ...bodyText(doc.body), ...authText(doc.auth)].join("\n");
  return unresolvedNames(text, info);
}

// ------------------------------------------------------------------------------------------- JSON

/** Replaces `{{var}}` outside strings by `0` so a templated body can still be checked. */
const placeholderJson = (text: string) => text.replace(VAR_RE, "0");

export type JsonStatus = { state: "empty" } | { state: "valid"; note?: string } | { state: "invalid"; message: string };

export function jsonStatus(text: string): JsonStatus {
  if (!text.trim()) return { state: "empty" };
  try {
    JSON.parse(text);
    return { state: "valid" };
  } catch (e) {
    try {
      JSON.parse(placeholderJson(text));
      return { state: "valid", note: "variables outside strings are checked as numbers" };
    } catch {
      return { state: "invalid", message: e instanceof Error ? e.message : "Invalid JSON" };
    }
  }
}

/** Pretty-printed JSON, or `null` when it does not parse (a body with bare variables is left alone). */
export function prettyJson(text: string): string | null {
  try {
    return JSON.stringify(JSON.parse(text), null, 2);
  } catch {
    return null;
  }
}

// ------------------------------------------------------------------------------------ type switches

export type BodyType = Body["type"];
export const BODY_TYPES: BodyType[] = ["none", "json", "text", "xml", "form", "multipart", "binary"];

export function switchBody(body: Body, type: BodyType): Body {
  if (body.type === type) return body;
  const content = body.type === "json" || body.type === "text" || body.type === "xml" ? body.content : "";
  switch (type) {
    case "none": return { type };
    case "json": case "text": case "xml": return { type, content };
    case "form": return { type, fields: body.type === "form" ? body.fields : [] };
    case "multipart": return { type, parts: [] };
    case "binary": return { type, file: "" };
  }
}

export type AuthType = Auth["type"];
export const AUTH_TYPES: AuthType[] = ["inherit", "none", "bearer", "basic", "apiKey"];

export function switchAuth(auth: Auth, type: AuthType): Auth {
  if (auth.type === type) return auth;
  switch (type) {
    case "inherit": case "none": return { type };
    case "bearer": return { type, token: "" };
    case "basic": return { type, username: "", password: "" };
    case "apiKey": return { type, name: "", value: "", in: "header" };
  }
}

/** How many entries a section holds, for its badge. */
export function sectionCounts(doc: RequestDoc) {
  const on = (rows: KeyValue[]) => rows.filter((r) => r.enabled && r.name).length;
  return {
    params: pathParamNames(doc.url).length + on(doc.query),
    headers: on(doc.headers),
    body: doc.body.type === "none" ? 0 : 1,
    auth: doc.auth.type === "inherit" || doc.auth.type === "none" ? 0 : 1,
    notes: doc.notes?.trim() ? 1 : 0,
  };
}

// ------------------------------------------------------------------------------------------ folders

export interface FolderOption {
  /** Folder slugs from the collection root; empty for the root itself. */
  path: string[];
  name: string;
  depth: number;
}

/** The collection root followed by every folder, depth-first, for a location picker. */
export function flattenFolders(children: TreeNode[], rootName: string): FolderOption[] {
  const out: FolderOption[] = [{ path: [], name: rootName, depth: 0 }];
  const walk = (nodes: TreeNode[], prefix: string[], depth: number) => {
    for (const n of nodes) {
      if (n.type !== "folder") continue;
      out.push({ path: [...prefix, n.slug], name: n.name, depth });
      walk(n.children, [...prefix, n.slug], depth + 1);
    }
  };
  walk(children, [], 1);
  return out;
}
