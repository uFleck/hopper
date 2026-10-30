import { describe, expect, it } from "vitest";
import type { RequestDto, RunResult } from "../ipc";
import {
  applyEdit,
  applyMove,
  deepEqual,
  draftFromDto,
  draftMeetsDisk,
  draftSaved,
  newRun,
  pruneRuns,
  runFailed,
  runFinished,
  revertDraft,
  runProgress,
} from "./reducers";

const dto = (over: Partial<RequestDto> = {}): RequestDto => ({
  doc: { id: "r1", name: "A", method: "GET", url: "/a", pathParams: {}, query: [], headers: [], body: { type: "none" }, auth: { type: "inherit" } },
  revision: "v1",
  collection: "c",
  folder: [],
  agentAccess: "ask",
  grantSuspended: false,
  readOnly: false,
  ...over,
});

describe("deepEqual", () => {
  it("ignores key order and undefined fields", () => {
    expect(deepEqual({ a: 1, b: { c: [1, 2] } }, { b: { c: [1, 2] }, a: 1 })).toBe(true);
    expect(deepEqual({ a: 1, b: undefined }, { a: 1 })).toBe(true);
    expect(deepEqual({ a: [1] }, { a: [1, 2] })).toBe(false);
    expect(deepEqual([], {})).toBe(false);
  });
});

describe("draft dirty tracking", () => {
  it("is clean on load, dirty after an edit, clean when edited back", () => {
    let d = draftFromDto(dto());
    expect(d.dirty).toBe(false);
    d = applyEdit(d, { name: "B" });
    expect(d.dirty).toBe(true);
    d = applyEdit(d, { name: "A" });
    expect(d.dirty).toBe(false);
  });

  it("a never-saved draft is dirty", () => {
    expect(draftFromDto(dto({ revision: null })).dirty).toBe(true);
  });

  it("moving collection or folder counts", () => {
    const d = applyMove(draftFromDto(dto()), "c", ["x"]);
    expect(d.dirty).toBe(true);
    expect(applyMove(d, "c", []).dirty).toBe(false);
  });

  it("read-only requests ignore edits", () => {
    const d = draftFromDto(dto({ readOnly: true }));
    expect(applyEdit(d, { name: "Z" })).toBe(d);
  });

  it("save adopts the new revision; edits typed meanwhile stay dirty", () => {
    let d = applyEdit(draftFromDto(dto()), { name: "B" });
    const sent = d.doc;
    d = applyEdit(d, { name: "C" });
    const saved = dto({ revision: "v2", doc: { ...sent } });
    d = draftSaved(d, saved, sent);
    expect(d.expectedRevision).toBe("v2");
    expect(d.doc.name).toBe("C");
    expect(d.dirty).toBe(true);
    const clean = draftSaved(applyEdit(draftFromDto(dto()), { name: "B" }), dto({ revision: "v2", doc: { ...dto().doc, name: "B" } }), { ...dto().doc, name: "B" });
    expect(clean.dirty).toBe(false);
    expect(clean.conflict).toBe(false);
  });
});

describe("revertDraft (Undo all)", () => {
  it("takes doc, collection and folder back to the baseline and is clean", () => {
    let d = applyEdit(draftFromDto(dto()), { name: "B", url: "/b" });
    d = applyMove(d, "other", ["f"]);
    expect(d.dirty).toBe(true);
    const r = revertDraft(d);
    expect(r.doc).toEqual(dto().doc);
    expect(r.collection).toBe("c");
    expect(r.folder).toEqual([]);
    expect(r.dirty).toBe(false);
  });

  it("leaves a never-saved draft untouched", () => {
    const d = applyEdit(draftFromDto(dto({ revision: null })), { name: "B" });
    expect(revertDraft(d)).toBe(d);
  });
});

describe("draftMeetsDisk", () => {
  it("same revision: untouched", () => {
    const d = draftFromDto(dto());
    expect(draftMeetsDisk(d, dto())).toBe(d);
  });
  it("clean draft reloads silently", () => {
    const d = draftFromDto(dto());
    const next = draftMeetsDisk(d, dto({ revision: "v2", doc: { ...dto().doc, name: "New" } }));
    expect(next.doc.name).toBe("New");
    expect(next.external).toBe(false);
  });
  it("dirty draft gets the banner and keeps its edits", () => {
    const d = applyEdit(draftFromDto(dto()), { name: "Mine" });
    const next = draftMeetsDisk(d, dto({ revision: "v2" }));
    expect(next.external).toBe(true);
    expect(next.doc.name).toBe("Mine");
  });
  it("deleted on disk flags a dirty draft; a draft being saved is left alone", () => {
    const d = applyEdit(draftFromDto(dto()), { name: "Mine" });
    expect(draftMeetsDisk(d, null).external).toBe(true);
    const saving = { ...d, saving: true };
    expect(draftMeetsDisk(saving, dto({ revision: "v9" }))).toBe(saving);
  });
});

describe("runs", () => {
  const result = { status: 200 } as RunResult;
  it("moves through phases and finishes", () => {
    let r = newRun("x", null, 1);
    r = runProgress(r, "sending");
    expect(r.phase).toBe("sending");
    r = runProgress(r, "done");
    expect(r.phase).toBe("sending");
    r = runFinished(r, result);
    expect(r.phase).toBe("done");
    expect(runProgress(r, "receiving").phase).toBe("done");
  });
  it("cancelled vs failed", () => {
    expect(runFailed(newRun("x", null, 1), { code: "cancelled", message: "" }).phase).toBe("cancelled");
    expect(runFailed(newRun("x", null, 1), { code: "boom", message: "" }).phase).toBe("failed");
  });
  it("prunes old finished runs but keeps in-flight ones", () => {
    const runs = Object.fromEntries(
      Array.from({ length: 5 }, (_, i) => [`r${i}`, i === 0 ? newRun(`r${i}`, null, i) : runFinished(newRun(`r${i}`, null, i), result)]),
    );
    const pruned = pruneRuns(runs, 2);
    expect(Object.keys(pruned).sort()).toEqual(["r0", "r3", "r4"]);
  });
});
