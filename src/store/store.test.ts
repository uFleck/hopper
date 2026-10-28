import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { hopperMock } from "../ipc/dev/mockBackend";
import { bus } from "../ipc/bus";
import { invoke } from "../ipc";
import * as A from "./actions";
import { mascotKind, resetStore, selectActiveEnv, store } from "./index";
import { confirm } from "../ui/overlays";

// No ConfirmHost is mounted in tests, so the in-app dialog is replaced by a stub the cases control.
vi.mock("../ui/overlays", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../ui/overlays")>()),
  confirm: vi.fn(async () => true),
}));

const state = () => store.getState();
const tick = () => new Promise((r) => setTimeout(r, 0));
async function settle(ms = 20) {
  await new Promise((r) => setTimeout(r, ms));
}

beforeEach(async () => {
  hopperMock.reset();
  resetStore();
  A.initStore();
  await vi.waitFor(() => expect(state().ready).toBe(true));
});
afterEach(() => resetStore());

describe("first load", () => {
  it("loads workspace, tree, environments, settings, mcp", () => {
    expect(state().workspace?.name).toBe("Umbler APIs");
    expect(state().tree?.collections[0].name).toBe("Account API");
    expect(state().environments.map((e) => e.stage)).toEqual(["local", "dev", "rc", "production"]);
    expect(selectActiveEnv(state())).toBe("local");
    expect(state().settings?.widthPreset).toBe(640);
    expect(state().mcp?.running).toBe(true);
    expect(state().recent.length).toBeGreaterThan(0);
  });

  it("loads settings even when no workspace is open", async () => {
    await invoke("workspace_close");
    resetStore();
    A.initStore();
    await vi.waitFor(() => expect(state().ready).toBe(true));
    expect(state().settings?.widthPreset).toBe(640);
    expect(state().workspace).toBeNull();
    expect(state().error).toBeNull();
  });

  it("tree has folders, an orphan and a suspended grant", () => {
    const flat: unknown[] = [];
    const walk = (nodes: { type: string; children?: unknown[] }[]) =>
      nodes.forEach((n) => (n.type === "folder" ? walk(n.children as never) : flat.push(n)));
    walk(state().tree!.collections[0].children as never);
    expect(flat.some((n) => (n as { orphaned: boolean }).orphaned)).toBe(true);
    expect(flat.some((n) => (n as { grantSuspended: boolean }).grantSuspended)).toBe(true);
  });
});

describe("environments", () => {
  it("setActiveEnv switches and the kind is production-red data", async () => {
    await A.setActiveEnv("production");
    expect(selectActiveEnv(state())).toBe("production");
    await A.setActiveEnv(null);
    expect(selectActiveEnv(state())).toBeNull();
  });
});

describe("request draft", () => {
  it("opens, tracks dirty, saves and clears dirty", async () => {
    await A.openRequest("01HX0000000000000000000001");
    expect(state().route).toBe("request");
    expect(state().draft?.dirty).toBe(false);
    A.editDraft({ name: "List all users" });
    expect(state().draft?.dirty).toBe(true);
    A.editDraft({ name: "List users" });
    expect(state().draft?.dirty).toBe(false);
    A.editDraft({ url: "{{baseUrl}}/api/users?x=1" });
    const before = state().draft!.expectedRevision;
    expect(await A.saveRequest()).toBe(true);
    expect(state().draft?.dirty).toBe(false);
    expect(state().draft?.expectedRevision).not.toBe(before);
  });

  it("an operation opens as an unsaved draft and save assigns an id", async () => {
    await A.openOperation("account-api", "account-openapi", "UserSessions_List");
    expect(state().draft?.doc.id).toBe("");
    expect(state().draft?.dirty).toBe(true);
    expect(state().draft?.doc.pathParams).toEqual({ id: "" });
    expect(await A.saveRequest()).toBe(true);
    expect(state().draft?.doc.id).not.toBe("");
    expect(state().draft?.dirty).toBe(false);
  });

  it("shows the conflict banner when the file moved on, and overwrite resolves it", async () => {
    await A.openRequest("01HX0000000000000000000002");
    // Someone else saves the same request.
    const other = await invoke("request_get", { id: "01HX0000000000000000000002" });
    await invoke("request_save", { doc: { ...other.doc, name: "Theirs" }, collection: "account-api", folder: ["users"] });
    A.editDraft({ name: "Mine" });
    expect(await A.saveRequest()).toBe(false);
    expect(state().draft?.conflict).toBe(true);
    await A.keepMine();
    expect(state().draft?.conflict).toBe(false);
    expect(await A.saveRequest()).toBe(true);
    expect((await invoke("request_get", { id: "01HX0000000000000000000002" })).doc.name).toBe("Mine");
  });

  it("a disk change reloads a clean draft and flags a dirty one", async () => {
    await A.openRequest("01HX0000000000000000000002");
    const other = await invoke("request_get", { id: "01HX0000000000000000000002" });
    await invoke("request_save", { doc: { ...other.doc, name: "Changed on disk" }, collection: "account-api", folder: ["users"] });
    await vi.waitFor(() => expect(state().draft?.doc.name).toBe("Changed on disk"));
    expect(state().draft?.external).toBe(false);

    A.editDraft({ notes: "mine" });
    await invoke("request_save", { doc: { ...other.doc, name: "Again" }, collection: "account-api", folder: ["users"] });
    await vi.waitFor(() => expect(state().draft?.external).toBe(true));
    expect(state().draft?.doc.notes).toBe("mine");
  });

  it("our own save does not raise the external-change banner", async () => {
    await A.openRequest("01HX0000000000000000000002");
    A.editDraft({ name: "Own save" });
    await A.saveRequest();
    await settle(60);
    expect(state().draft?.external).toBe(false);
    expect(state().draft?.dirty).toBe(false);
  });

  it("delete closes the request and the undo toast restores it", async () => {
    await A.openRequest("01HX0000000000000000000006");
    await A.deleteRequest("01HX0000000000000000000006");
    expect(state().draft).toBeNull();
    expect(state().route).toBe("find");
    const t = state().toasts[state().toasts.length - 1];
    expect(t.undo).toBeDefined();
    await t.undo!.run();
    expect(JSON.stringify(state().tree)).toContain("Slow report");
  });
});

describe("send", () => {
  it("sends a saved request and records the result, history and recent", async () => {
    await A.openRequest("01HX0000000000000000000002");
    const res = await A.send();
    expect(res?.status).toBe(200);
    const run = state().runs[state().draft!.runId!];
    expect(run.phase).toBe("done");
    expect(run.result?.resolvedUrl).toBe("http://localhost:5080/api/users/42");
    await vi.waitFor(() => expect(state().history["01HX0000000000000000000002"].length).toBeGreaterThan(1));
  });

  it("returns 404 and html bodies", async () => {
    await A.openRequest("01HX0000000000000000000007");
    expect((await A.send())?.status).toBe(404);
    await A.openRequest("01HX0000000000000000000008");
    expect((await A.send())?.kind).toBe("html");
  });

  it("refuses unresolved variables with a HopperError", async () => {
    await A.setActiveEnv(null);
    await A.openRequest("01HX0000000000000000000001");
    expect(await A.send()).toBeNull();
    const run = state().runs[state().draft!.runId!];
    expect(run.phase).toBe("failed");
    expect(run.error?.code).toBe("unresolved_variables");
    expect(run.error?.details?.names).toContain("baseUrl");
  });

  it("refuses a missing secret", async () => {
    await A.setActiveEnv("rc");
    await A.openRequest("01HX0000000000000000000009");
    expect(await A.send()).toBeNull();
    expect(state().runs[state().draft!.runId!].error?.code).toBe("secret_missing");
  });

  it("progress events move the phase, cancel ends the run as cancelled", async () => {
    await A.openRequest("01HX0000000000000000000006"); // the slow one
    const p = A.send();
    await vi.waitFor(() => expect(state().draft?.runId).toBeTruthy());
    const runId = state().draft!.runId!;
    await vi.waitFor(() => expect(state().runs[runId].phase).toBe("sending"));
    await A.cancel();
    expect(await p).toBeNull();
    expect(state().runs[runId].phase).toBe("cancelled");
  });

  it("an unlisted host asks first, and confirming retries the same send with confirmedHost", async () => {
    vi.mocked(confirm).mockResolvedValueOnce(true);
    await A.openRequest("01HX0000000000000000000002");
    const res = await A.send({ baseUrl: "https://evil.example.com" });
    expect(res?.status).toBe(200);
    expect(res?.resolvedUrl).toBe("https://evil.example.com/api/users/42");
    expect(vi.mocked(confirm)).toHaveBeenCalledWith(expect.objectContaining({ title: "Send secrets to an unlisted host?", tone: "danger" }));
    expect(vi.mocked(confirm).mock.calls[0][0].message).toContain("evil.example.com");
    expect(state().runs[state().draft!.runId!].phase).toBe("done");
  });

  it("declining the unlisted host ends the run as cancelled without an error", async () => {
    vi.mocked(confirm).mockResolvedValueOnce(false);
    await A.openRequest("01HX0000000000000000000002");
    expect(await A.send({ baseUrl: "https://evil.example.com" })).toBeNull();
    const run = state().runs[state().draft!.runId!];
    expect(run.phase).toBe("cancelled");
    expect(run.error).toBeNull();
  });

  it("a dirty saved request is sent as a draft", async () => {
    await A.openRequest("01HX0000000000000000000002");
    A.editDraft({ url: "{{baseUrl}}/api/missing" });
    expect((await A.send())?.status).toBe(404);
  });
});

describe("events", () => {
  it("endpoints an agent adds become unseen notices until the user looks", () => {
    hopperMock.endpoints({ label: "POST /users" });
    hopperMock.endpoints({ label: "GET /users/{id}" });
    expect(state().notices.map((n) => [n.label, n.seen])).toEqual([
      ["GET /users/{id}", false],
      ["POST /users", false],
    ]);
    A.markNoticesSeen();
    expect(state().notices.every((n) => n.seen)).toBe(true);
  });

  it("approval requests arrive, mascot says approval, respond clears them", async () => {
    const a = hopperMock.requestApproval();
    expect(state().approvals.map((x) => x.id)).toEqual([a.id]);
    expect(mascotKind(state())).toBe("approval");
    await A.respondApproval(a.id, "allow_once");
    expect(state().approvals).toEqual([]);
    expect(state().activity[0].allowedBy).toBe("approved by user");
  });

  it("activity events prepend without duplicates; the mcp flip is re-read", async () => {
    const before = state().activity.length;
    hopperMock.activity({ target: "x" });
    expect(state().activity.length).toBe(before + 1);
    hopperMock.setMcp({ running: false, error: "port busy" });
    await vi.waitFor(() => expect(state().mcp?.running).toBe(false));
    expect(mascotKind(state())).toBe("agent");
  });

  it("resolved events remove an approval raised elsewhere", () => {
    const a = hopperMock.requestApproval();
    bus.emit("agent://approval-resolved", { id: a.id });
    expect(state().approvals).toEqual([]);
  });

  it("show-request opens the request", async () => {
    bus.emit("panel://show-request", { id: "01HX0000000000000000000003" });
    await vi.waitFor(() => expect(state().draft?.doc.name).toBe("Create invite"));
    expect(state().route).toBe("request");
  });

  it("secrets status refreshes environments", async () => {
    await invoke("secret_set", { stage: "rc", application: "account", name: "adminKey", value: "x" });
    await vi.waitFor(() =>
      expect(state().environments.find((e) => e.stage === "rc")?.applications[0]?.secrets.find((s) => s.name === "adminKey")?.status).toBe("set"),
    );
  });

  it("initStore twice does not double-handle events", async () => {
    A.initStore();
    await vi.waitFor(() => expect(state().ready).toBe(true));
    hopperMock.requestApproval();
    expect(state().approvals.length).toBe(1);
  });
});

describe("sheets, route, settings", () => {
  it("opens and closes a sheet, updates settings", async () => {
    A.openSheet("settings");
    expect(state().sheet).toBe("settings");
    A.closeSheet();
    expect(state().sheet).toBeNull();
    await A.updateSettings({ widthPreset: 640 });
    expect(state().settings?.widthPreset).toBe(640);
  });

  it("pending run is visible to selectors only while in flight", async () => {
    await tick();
    expect(state().runs).toEqual({});
  });
});
