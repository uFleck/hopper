import { beforeEach, describe, expect, it } from "vitest";
import type { TreeDto } from "../ipc";
import { collectionKey, folderKey, getOpenNodes, nodeKeys, resetTreeOpen, setNodes, toggleNode } from "./treeOpen";

const request = { type: "request", id: "r", name: "R", method: "GET", agentAccess: "ask", grantSuspended: false, orphaned: false } as const;
const tree: TreeDto = {
  problems: [],
  collections: [
    { slug: "acct", name: "Account", specs: [], children: [{ type: "folder", slug: "users", name: "Users", children: [{ type: "folder", slug: "admin", name: "Admin", children: [request] }] }, request] },
    { slug: "bill", name: "Bill", specs: [], children: [] },
  ],
};

describe("tree open state", () => {
  beforeEach(resetTreeOpen);

  it("lists each collection and every nested folder", () => {
    expect(nodeKeys(tree)).toEqual(["c:acct", "f:acct/users", "f:acct/users/admin", "c:bill"]);
    expect(nodeKeys(null)).toEqual([]);
  });

  it("toggles one node without touching the others", () => {
    toggleNode(collectionKey("acct"));
    toggleNode(folderKey("acct", ["users"]));
    toggleNode(collectionKey("acct"));
    expect([...getOpenNodes()]).toEqual(["f:acct/users"]);
  });

  it("collapse all leaves nodes of other workspaces alone", () => {
    toggleNode(collectionKey("elsewhere"));
    setNodes(nodeKeys(tree), true);
    expect(getOpenNodes().size).toBe(5);
    setNodes(nodeKeys(tree), false);
    expect([...getOpenNodes()]).toEqual(["c:elsewhere"]);
  });
});
