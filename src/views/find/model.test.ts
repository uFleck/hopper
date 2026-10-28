import { describe, expect, it } from "vitest";
import type { TreeDto } from "../../ipc";
import { flattenRequests, folderOptions, highlightSegments, nameFromPath, requestIdsIn } from "./model";

const tree: TreeDto = {
  problems: [],
  collections: [
    {
      slug: "account",
      name: "Account",
      specs: [],
      children: [
        { type: "request", id: "r1", name: "Root", method: "GET", agentAccess: "ask", grantSuspended: false, orphaned: false },
        {
          type: "folder",
          slug: "users",
          name: "Users",
          children: [
            { type: "request", id: "r2", name: "List", method: "GET", agentAccess: "auto", grantSuspended: false, orphaned: false },
            { type: "request", id: "r3", name: "Create", method: "POST", agentAccess: "ask", grantSuspended: true, orphaned: true },
            { type: "folder", slug: "admin", name: "Admin", children: [] },
          ],
        },
      ],
    },
  ],
};

describe("highlightSegments", () => {
  it("groups adjacent matched characters", () => {
    expect(highlightSegments("/api/users", [1, 2, 3, 5])).toEqual([
      { text: "/", hit: false },
      { text: "api", hit: true },
      { text: "/", hit: false },
      { text: "u", hit: true },
      { text: "sers", hit: false },
    ]);
  });

  it("returns one plain segment without matches", () => {
    expect(highlightSegments("/x", [])).toEqual([{ text: "/x", hit: false }]);
  });
});

describe("tree helpers", () => {
  it("flattens requests with their location", () => {
    const flat = flattenRequests(tree);
    expect(flat.map((r) => [r.id, r.collection, r.folder.join("/")])).toEqual([
      ["r1", "account", ""],
      ["r2", "account", "users"],
      ["r3", "account", "users"],
    ]);
  });

  it("lists move targets with readable labels", () => {
    expect(folderOptions(tree).map((o) => o.label)).toEqual(["Account", "Account / Users", "Account / Users / Admin"]);
  });

  it("collects request ids by predicate, recursively", () => {
    expect(requestIdsIn(tree.collections[0].children)).toEqual(["r1", "r2", "r3"]);
    expect(requestIdsIn(tree.collections[0].children, (r) => r.method === "GET")).toEqual(["r1", "r2"]);
  });

  it("names a workspace after its folder", () => {
    expect(nameFromPath("C:\\Work\\umbler-apis\\")).toBe("umbler-apis");
    expect(nameFromPath("/home/me/api")).toBe("api");
  });
});
