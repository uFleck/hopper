import { describe, expect, it } from "vitest";
import { hopperMock } from "../ipc/dev/mockBackend";
import * as A from "./actions";
import { resetStore, store } from "./index";
import { scopeLayers, scopeTargets, targetKey, withVariable } from "./scopes";

describe("scope targets", () => {
  it("lists workspace, collection, each folder from the root, then the request", () => {
    const t = scopeTargets("acct", ["users", "admin"], "r1");
    expect(t.map((x) => x.origin)).toEqual(["workspace", "collection acct", "folder users", "folder users/admin", "this request"]);
    expect(t[3].target).toEqual({ type: "folder", collection: "acct", path: ["users", "admin"] });
  });

  it("has only the workspace for a draft without a collection", () => {
    expect(scopeTargets(null, [], null).map((x) => x.origin)).toEqual(["workspace"]);
  });

  it("puts the workspace under the environment and the rest over it", () => {
    const targets = scopeTargets("acct", [], null);
    const loaded = Object.fromEntries(targets.map((t) => [targetKey(t.target), { target: t.target, variables: [{ name: "k", value: t.origin }], revision: "r" }]));
    const layers = scopeLayers(loaded, targets);
    expect(layers.below.map((l) => l.origin)).toEqual(["workspace"]);
    expect(layers.above.map((l) => l.origin)).toEqual(["collection acct"]);
  });

  it("replaces a variable in place or appends it", () => {
    expect(withVariable([{ name: "a", value: "1" }], "a", "2")).toEqual([{ name: "a", value: "2" }]);
    expect(withVariable([{ name: "a", value: "1" }], "b", "2")).toEqual([{ name: "a", value: "1" }, { name: "b", value: "2" }]);
  });
});

describe("saving variables", () => {
  it("adds one on top of what is saved and keeps the store in step", async () => {
    hopperMock.reset();
    resetStore();
    const target = { type: "collection", slug: "account-api" } as const;
    expect(await A.setVariable(target, "a", "1")).toBe(true);
    expect(await A.setVariable(target, "b", "2")).toBe(true);
    expect(store.getState().scopeVars[targetKey(target)].variables).toEqual([{ name: "a", value: "1" }, { name: "b", value: "2" }]);
    expect((await A.loadVariables(target))?.variables).toHaveLength(2);
  });

  it("refuses a stale save", async () => {
    hopperMock.reset();
    resetStore();
    const target = { type: "collection", slug: "account-api" } as const;
    await A.setVariable(target, "a", "1");
    expect(await A.saveVariables(target, [], "vars-stale")).toBe(false);
  });
});
