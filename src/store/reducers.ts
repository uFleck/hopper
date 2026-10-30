/* Pure state logic: no invoke, no timers. The actions in ./actions.ts call these. */
import type { HopperError, RequestDoc, RequestDto, RunResult } from "../ipc";

export type SheetName = "collections" | "environments" | "agent" | "settings";
export type AgentTab = "activity" | "approvals" | "grants" | "connect";
export type Route = "find" | "request";

/** The request open in the editor: the working copy next to the baseline it was loaded or last saved from. */
export interface DraftState {
  /** As last loaded or saved; `revision === null` means it has never been saved. */
  base: RequestDto;
  doc: RequestDoc;
  collection: string | null;
  folder: string[];
  /** Differs from the baseline, or was never saved. */
  dirty: boolean;
  /** Sent back as `expectedRevision` on save; `null` for a brand-new draft. */
  expectedRevision: string | null;
  saving: boolean;
  saveError: HopperError | null;
  /** Save was refused with `revision_conflict`: the banner offers reload or overwrite. */
  conflict: boolean;
  /** The file changed on disk while the draft was dirty: the banner offers reload or keep mine. */
  external: boolean;
  runId: string | null;
  /** Opened from a history run: `unsaved` has no saved version yet, `no-request` could not restore the request that was sent. */
  fromHistory?: "unsaved" | "no-request";
}

export type RunPhase = "preparing" | "sending" | "receiving" | "done" | "cancelled" | "failed";

export interface RunState {
  runId: string;
  requestId: string | null;
  phase: RunPhase;
  result: RunResult | null;
  error: HopperError | null;
  startedAt: number;
}

// ------------------------------------------------------------------------------------------- equality

/** Structural equality that ignores object key order and treats `undefined` fields as absent. */
export function deepEqual(a: unknown, b: unknown): boolean {
  if (Object.is(a, b)) return true;
  if (typeof a !== "object" || typeof b !== "object" || a === null || b === null) return false;
  if (Array.isArray(a) !== Array.isArray(b)) return false;
  if (Array.isArray(a) && Array.isArray(b)) return a.length === b.length && a.every((v, i) => deepEqual(v, b[i]));
  const ao = a as Record<string, unknown>;
  const bo = b as Record<string, unknown>;
  const keys = new Set([...Object.keys(ao), ...Object.keys(bo)]);
  for (const k of keys) if (!deepEqual(ao[k], bo[k])) return false;
  return true;
}

// ------------------------------------------------------------------------------------------ the draft

export function draftFromDto(dto: RequestDto): DraftState {
  return {
    base: dto,
    doc: dto.doc,
    collection: dto.collection,
    folder: dto.folder,
    dirty: dto.revision === null,
    expectedRevision: dto.revision,
    saving: false,
    saveError: null,
    conflict: false,
    external: false,
    runId: null,
  };
}

export function isDirty(d: Pick<DraftState, "base" | "doc" | "collection" | "folder">): boolean {
  return (
    d.base.revision === null ||
    !deepEqual(d.base.doc, d.doc) ||
    d.base.collection !== d.collection ||
    !deepEqual(d.base.folder, d.folder)
  );
}

/** Applies an edit to the working copy and recomputes the dirty flag. Read-only requests ignore edits. */
export function applyEdit(d: DraftState, patch: Partial<RequestDoc>): DraftState {
  if (d.base.readOnly) return d;
  const next = { ...d, doc: { ...d.doc, ...patch } };
  return { ...next, dirty: isDirty(next) };
}

export function applyMove(d: DraftState, collection: string | null, folder: string[]): DraftState {
  const next = { ...d, collection, folder };
  return { ...next, dirty: isDirty(next) };
}

/** Undo all: the working copy goes back to the baseline. A never-saved draft has none, so it is left alone. */
export function revertDraft(d: DraftState): DraftState {
  if (d.base.revision === null) return d;
  const next = { ...d, doc: d.base.doc, collection: d.base.collection, folder: d.base.folder };
  return { ...next, dirty: isDirty(next) };
}

/** The server accepted the save: the response becomes the new baseline (unless the user typed more while it was in flight). */
export function draftSaved(d: DraftState, saved: RequestDto, sentDoc: RequestDoc): DraftState {
  const editedMeanwhile = !deepEqual(d.doc, sentDoc);
  const next: DraftState = {
    ...d,
    base: saved,
    doc: editedMeanwhile ? d.doc : saved.doc,
    collection: saved.collection,
    folder: saved.folder,
    expectedRevision: saved.revision,
    saving: false,
    saveError: null,
    conflict: false,
    external: false,
    fromHistory: undefined,
  };
  return { ...next, dirty: isDirty(next) };
}

/** A fresh server copy arrived while the draft is open. Returns it replaced when clean, or flagged `external` when dirty. */
export function draftMeetsDisk(d: DraftState, disk: RequestDto | null): DraftState {
  if (d.saving) return d;
  if (disk && disk.revision === d.expectedRevision) return d;
  if (d.base.revision === null) return d; // never saved: nothing on disk to disagree with
  if (!d.dirty && disk) return { ...draftFromDto(disk), runId: d.runId };
  return d.external ? d : { ...d, external: true };
}

// ---------------------------------------------------------------------------------------------- runs

export function newRun(runId: string, requestId: string | null, now: number): RunState {
  return { runId, requestId, phase: "preparing", result: null, error: null, startedAt: now };
}

export function runProgress(r: RunState, phase: "preparing" | "sending" | "receiving" | "done"): RunState {
  // A run that already finished (result, failure, cancel) never goes back.
  if (r.phase === "done" || r.phase === "cancelled" || r.phase === "failed") return r;
  if (phase === "done") return r; // `done` is set when the result arrives
  return { ...r, phase };
}

export function runFinished(r: RunState, result: RunResult): RunState {
  return { ...r, phase: "done", result, error: null };
}

export function runFailed(r: RunState, error: HopperError): RunState {
  return { ...r, phase: error.code === "cancelled" ? "cancelled" : "failed", error };
}

/** Prunes finished runs, keeping the newest `keep` plus anything still in flight. */
export function pruneRuns(runs: Record<string, RunState>, keep = 20): Record<string, RunState> {
  const all = Object.values(runs);
  const finished = all.filter((r) => r.phase === "done" || r.phase === "cancelled" || r.phase === "failed");
  if (finished.length <= keep) return runs;
  const drop = new Set(finished.sort((a, b) => b.startedAt - a.startedAt).slice(keep).map((r) => r.runId));
  return Object.fromEntries(Object.entries(runs).filter(([id]) => !drop.has(id)));
}
