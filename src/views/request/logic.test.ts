import { describe, expect, it } from "vitest";
import type { RequestDoc, VarTargetDto } from "../../ipc";
import { allUnresolved, closestNames, resolveEnv, type ResolvedEnv, flattenFolders, jsonStatus, MASK, pathParamNames, prettyJson, renameVariable, resolveUrl, sectionCounts, suggest, switchBody, varInfo, variableAt, variableName, variableTokenAt, variablesIn } from "./logic";
import { chunkEnd, formatBytes, formatMs, mimeType, sizeLabel, timeAgo } from "../response/format";

const env = (): ResolvedEnv => ({
  name: "rc",
  variables: [{ name: "baseUrl", value: "https://api.test", local: false }, { name: "userId", value: "7", local: false }],
  secrets: [{ name: "apiToken", provider: "keyring", status: "set" }, { name: "adminKey", provider: "keyring", status: "missing" }],
  tls: { insecureSkipVerify: false },
  readOnly: false,
  applicationOf: new Map([["baseUrl", "api"], ["userId", "api"]]),
});

const doc = (over: Partial<RequestDoc> = {}): RequestDoc => ({
  id: "x", name: "n", method: "GET", url: "{{baseUrl}}/users/{id}", pathParams: { id: "{{userId}}" }, query: [], headers: [],
  body: { type: "none" }, auth: { type: "inherit" }, ...over,
});

describe("resolveEnv", () => {
  it("uses the application matching the collection, else all sections of the stage", () => {
    const sec = (name: string) => ({ name, provider: "keyring", status: "set" as const });
    const app = (application: string, v: string) => ({ application, variables: [{ name: v, value: "1", local: false }], secrets: [sec(`${v}Key`)], tls: { insecureSkipVerify: false }, allowedHosts: [], allowedHostsExplicit: false });
    const stage = { stage: "rc" as const, active: true, applications: [app("a", "x"), app("b", "y")], revision: "r", readOnly: false, agentRules: { deny: false, safeReads: true, patterns: [] } };
    expect(resolveEnv(stage, "b")?.variables.map((v) => v.name)).toEqual(["y"]);
    expect(resolveEnv(stage, "zzz")?.variables.map((v) => v.name)).toEqual(["x", "y"]);
    expect(resolveEnv(undefined, "a")).toBeNull();
  });
});

describe("variables", () => {
  it("finds variables and path params without mixing them up", () => {
    expect(variablesIn("{{a}}/x/{{ b }}/{{a}}")).toEqual(["a", "b"]);
    expect(pathParamNames("{{baseUrl}}/users/{id}/roles/{roleId}")).toEqual(["id", "roleId"]);
    expect(pathParamNames("{{baseUrl}}/x")).toEqual([]);
  });

  it("resolves the URL, masks secrets and names what is missing", () => {
    const info = varInfo(env());
    const r = resolveUrl(doc({ query: [{ name: "q", value: "{{apiToken}}", enabled: true }, { name: "off", value: "1", enabled: false }] }), info);
    expect(r.text).toBe(`https://api.test/users/7?q=${MASK}`);
    expect(r.emptyPathParams).toEqual([]);
    const u = resolveUrl(doc({ url: "{{nope}}/a/{id}", pathParams: {} }), info);
    expect(u.text).toBe("{{nope}}/a/{id}");
    expect(u.emptyPathParams).toEqual(["id"]);
    expect(u.segments.find((s) => s.kind === "unresolved")?.text).toBe("{{nope}}");
    expect(resolveUrl(doc({ url: "{{adminKey}}" }), info).segments[0].kind).toBe("missing");
  });

  it("masks an API key sent in the query and lists unresolved names across the request", () => {
    const info = varInfo(env());
    const d = doc({ auth: { type: "apiKey", name: "k", value: "{{gone}}", in: "query" }, headers: [{ name: "X", value: "{{other}}", enabled: true }, { name: "Y", value: "{{off}}", enabled: false }] });
    expect(resolveUrl(d, info).text.endsWith(`?k=${MASK}`)).toBe(true);
    expect(allUnresolved(d, info)).toEqual(["other", "gone"]);
    expect(allUnresolved(doc({ body: { type: "json", content: '{"t":"{{$guid}}"}' } }), info)).toEqual([]);
  });
});

describe("suggestions", () => {
  const info = varInfo(env(), { below: [], above: [{ origin: "collection a", target: { type: "collection", slug: "a" }, vars: [{ name: "tenantId", value: "t1" }] }] }, "a");

  it("lists everything on an empty query, nearest scope first and built-ins last", () => {
    const names = suggest(info, "").map((s) => s.name);
    expect(names).toEqual(["tenantId", "baseUrl", "userId", "adminKey", "apiToken", "$guid", "$isoTimestamp", "$randomInt", "$timestamp"]);
    expect(suggest(info, "").find((s) => s.name === "adminKey")?.detail).toBe("secret, no value yet");
  });

  it("ranks prefix over word start over inside, and drops what does not match", () => {
    expect(suggest(info, "api").map((s) => s.name)).toEqual(["apiToken"]);
    expect(suggest(info, "url").map((s) => s.name)).toEqual(["baseUrl"]);
    expect(suggest(info, "bsu").map((s) => s.name)).toEqual(["baseUrl"]);
    expect(suggest(info, "id").map((s) => s.name)).toEqual(["tenantId", "userId", "$guid"]);
    expect(suggest(info, "zzz")).toEqual([]);
    expect(suggest(info, "$time")[0].name).toBe("$timestamp");
  });

  it("offers a defined name for a typo, and only close ones", () => {
    expect(closestNames("baseUrll", info)).toEqual(["baseUrl"]);
    expect(closestNames("apitoken", info)).toEqual(["apiToken"]);
    expect(closestNames("completelyOther", info)).toEqual([]);
    expect(closestNames("$timestmp", info)).toEqual(["$timestamp"]);
  });

  it("renames every token of a variable, whatever its spacing, and nothing else", () => {
    expect(renameVariable("{{a}}/{{ a }}/{{ab}}/a", "a", "b")).toBe("{{b}}/{{b}}/{{ab}}/a");
  });

  it("finds the token under a character with its bounds", () => {
    expect(variableTokenAt("x {{ id }} y", 5)).toEqual({ name: "id", from: 2, to: 10 });
    expect(variableTokenAt("x {{ id }} y", 11)).toBeNull();
  });
});

describe("scoped variables", () => {
  const layer = (origin: string, ...pairs: [string, string][]) => {
    const [kind, name] = origin.split(" ");
    const target: VarTargetDto = kind === "collection" ? { type: "collection", slug: name } : kind === "folder" ? { type: "folder", collection: "a", path: name.split("/") } : { type: "workspace" };
    return { origin, target, vars: pairs.map(([name, value]) => ({ name, value })) };
  };

  it("layers request > folder > collection > environment > workspace and remembers the origin", () => {
    const info = varInfo(env(), {
      below: [layer("workspace", ["baseUrl", "ws"], ["wsOnly", "w"])],
      above: [layer("collection a", ["userId", "col"], ["tier", "col"]), layer("folder x", ["tier", "folder"])],
    });
    expect(info.values.get("baseUrl")).toBe("https://api.test");
    expect(info.origins.get("baseUrl")).toBe("rc environment");
    expect(info.values.get("userId")).toBe("col");
    expect(info.values.get("tier")).toBe("folder");
    expect(info.origins.get("tier")).toBe("folder x");
    expect(info.origins.get("wsOnly")).toBe("workspace");
  });

  it("types each source and says where an edit is written", () => {
    const info = varInfo(env(), { below: [layer("workspace", ["wsOnly", "w"])], above: [layer("collection a", ["userId", "col"]), layer("folder x/y", ["tier", "f"])] });
    expect(info.sources.get("baseUrl")).toEqual({ kind: "environment", detail: "rc", edit: { type: "environment", stage: "rc", application: "api" } });
    expect(info.sources.get("userId")).toEqual({ kind: "collection", detail: "a", edit: { type: "scope", target: { type: "collection", slug: "a" } } });
    expect(info.sources.get("tier")).toMatchObject({ kind: "folder", detail: "x/y" });
    expect(info.sources.get("wsOnly")).toMatchObject({ kind: "workspace", edit: { type: "scope", target: { type: "workspace" } } });
    expect(varInfo({ ...env(), readOnly: true }).sources.get("baseUrl")?.edit).toBeNull();
  });

  it("stops flagging a name once a scope defines it", () => {
    const d = doc({ url: "{{fromCollection}}/a" });
    expect(allUnresolved(d, varInfo(env()))).toEqual(["fromCollection"]);
    expect(allUnresolved(d, varInfo(env(), { below: [], above: [layer("collection a", ["fromCollection", "1"])] }))).toEqual([]);
  });

  it("finds the variable under a character", () => {
    const text = "a/{{ id }}/b/{{x}}";
    expect(variableAt(text, 0)).toBeNull();
    expect(variableAt(text, 2)).toBe("id");
    expect(variableAt(text, 9)).toBe("id");
    expect(variableAt(text, 10)).toBeNull();
    expect(variableAt(text, 16)).toBe("x");
  });

  it("reads the name out of a {{ token }}", () => {
    expect(variableName("{{ id }}")).toBe("id");
    expect(variableName("{{id}}")).toBe("id");
  });
});

describe("json body", () => {
  it("validates, tolerating variables outside strings", () => {
    expect(jsonStatus("").state).toBe("empty");
    expect(jsonStatus('{"a":1}')).toEqual({ state: "valid" });
    expect(jsonStatus('{"a":{{n}}}').state).toBe("valid");
    expect(jsonStatus("{a:1}").state).toBe("invalid");
  });

  it("pretty-prints only valid JSON", () => {
    expect(prettyJson('{"a":[1,2]}')).toBe('{\n  "a": [\n    1,\n    2\n  ]\n}');
    expect(prettyJson("{nope")).toBeNull();
  });
});

describe("type switches and counts", () => {
  it("keeps text content across json/text/xml and drops it elsewhere", () => {
    expect(switchBody({ type: "json", content: "{}" }, "xml")).toEqual({ type: "xml", content: "{}" });
    expect(switchBody({ type: "json", content: "{}" }, "form")).toEqual({ type: "form", fields: [] });
  });

  it("counts enabled entries", () => {
    const c = sectionCounts(doc({ query: [{ name: "a", value: "", enabled: true }, { name: "", value: "", enabled: true }], notes: "x", auth: { type: "bearer", token: "t" } }));
    expect(c).toMatchObject({ params: 2, notes: 1, auth: 1, body: 0, headers: 0 });
  });
});

describe("folders", () => {
  it("lists the root then folders depth-first, skipping requests", () => {
    const tree = [
      { type: "folder" as const, slug: "users", name: "Users", children: [{ type: "folder" as const, slug: "admin", name: "Admin", children: [] }, { type: "request" as const, id: "1", name: "r", method: "GET" as const, agentAccess: "ask" as const, grantSuspended: false, orphaned: false }] },
      { type: "folder" as const, slug: "roles", name: "Roles", children: [] },
    ];
    expect(flattenFolders(tree, "Account").map((f) => [f.path.join("/"), f.depth])).toEqual([["", 0], ["users", 1], ["users/admin", 2], ["roles", 1]]);
  });
});

describe("response formatting", () => {
  it("formats sizes, times and age", () => {
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(2048)).toBe("2.0 KB");
    expect(formatBytes(3 * 1024 * 1024)).toBe("3.0 MB");
    expect(formatMs(142)).toBe("142 ms");
    expect(formatMs(2410)).toBe("2.41 s");
    expect(timeAgo(new Date(0).toISOString(), 3 * 60_000)).toBe("3 min ago");
    expect(sizeLabel(100)).toBe("100 B");
    expect(sizeLabel(4096, 1024)).toBe("1.0 KB transferred, 4.0 KB decoded");
  });

  it("reads the mime type and chunks at line breaks", () => {
    expect(mimeType([["Content-Type", "image/PNG; x=1"]])).toBe("image/png");
    expect(chunkEnd("aaa\nbbb\nccc", 0, 6)).toBe(4);
    expect(chunkEnd("abc", 0, 10)).toBe(3);
    expect(chunkEnd("abcdefghij", 0, 4)).toBe(4);
  });
});
