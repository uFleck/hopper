import { beforeEach, describe, expect, it } from "vitest";
import { invoke, isHopperError, toHopperError } from "./index";
import { hopperMock } from "./dev/mockBackend";
import { parseQuery } from "./dev/mock/search";

beforeEach(() => hopperMock.reset());

const find = (query: string) => invoke("search_routes", { query });

describe("search_routes", () => {
  it("parses the query syntax", () => {
    expect(parseQuery("post users")).toEqual({ words: ["users"], method: "POST" });
    expect(parseQuery("tag:UserAdmin list")).toEqual({ words: ["list"], tag: "useradmin" });
    expect(parseQuery("#GetUserById")).toEqual({ words: [], opId: "getuserbyid" });
    expect(parseQuery("path:/api/users/{id}")).toEqual({ words: [], path: "/api/users/{id}" });
  });

  it("ANDs fuzzy tokens and returns match indices", async () => {
    const r = await find("usr sess");
    const hit = r.hits[0];
    expect(hit.kind).toBe("operation");
    expect(hit.path).toContain("sessions");
    expect(hit.matches.length).toBeGreaterThan(3);
    expect(r.hits.slice(0, 3).every((h) => h.path.includes("sessions"))).toBe(true);
  });

  it("method prefix, tag and operation id", async () => {
    const post = await find("post users");
    expect(post.hits.every((h) => h.method === "POST")).toBe(true);
    expect(post.methods).toEqual([{ method: "POST", count: post.total }]);
    const tag = await find("tag:UserRoles");
    expect(tag.hits.length).toBeGreaterThanOrEqual(5);
    expect(tag.hits.every((h) => h.kind === "operation" && h.tags.includes("UserRoles"))).toBe(true);
    const op = await find("#UserSessions_Revoke");
    expect(op.hits.map((h) => h.kind === "operation" && h.key)).toContain("UserSessions_Revoke");
  });

  it("marks operations already saved and sinks deprecated ones", async () => {
    const r = await find("users");
    const op = r.hits.find((h) => h.kind === "operation" && h.key === "UserAdmin_List");
    expect(op && op.kind === "operation" && op.savedRequestId).toBeTruthy();
    const idx = (key: string) => r.hits.findIndex((h) => h.kind === "operation" && h.key === key);
    expect(idx("Legacy_ExportUsers")).toBeGreaterThan(idx("UserAdmin_List"));
    expect(idx("Legacy_ExportUsers")).toBeGreaterThan(-1);
  });

  it("facets cover the full match set, not the page", async () => {
    const r = await invoke("search_routes", { query: "user", limit: 3 });
    expect(r.hits).toHaveLength(3);
    expect(r.total).toBeGreaterThan(20);
    expect(r.tags.reduce((n, t) => n + t.count, 0)).toBeGreaterThan(20);
  });
});

describe("errors", () => {
  it("normalises rejections to HopperError", () => {
    expect(toHopperError("boom")).toEqual({ code: "internal", message: "boom" });
    expect(toHopperError({ code: "x", message: "m", hint: "h" })).toEqual({ code: "x", message: "m", hint: "h" });
    expect(toHopperError(new Error("e"))).toEqual({ code: "internal", message: "e" });
  });

  it("commands reject with HopperError", async () => {
    const e = await invoke("request_get", { id: "nope" }).catch((x) => x);
    expect(isHopperError(e, "not_found")).toBe(true);
  });
});
