import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { hopperMock } from "../ipc/dev/mockBackend";
import * as ipc from "../ipc";
import { debounced, latestWins } from "./latest";
import * as A from "./actions";
import { resetStore, store } from "./index";

describe("debounced / latestWins", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("collapses bursts into one call", () => {
    const fn = vi.fn();
    const d = debounced(fn, 100);
    d.schedule();
    vi.advanceTimersByTime(60);
    d.schedule();
    vi.advanceTimersByTime(60);
    expect(fn).not.toHaveBeenCalled();
    vi.advanceTimersByTime(40);
    expect(fn).toHaveBeenCalledTimes(1);
  });

  it("cancel drops the pending call, flush runs it now", () => {
    const fn = vi.fn();
    const d = debounced(fn, 100);
    d.schedule();
    d.cancel();
    vi.advanceTimersByTime(200);
    expect(fn).not.toHaveBeenCalled();
    d.schedule();
    d.flush();
    expect(fn).toHaveBeenCalledTimes(1);
  });

  it("only the newest ticket is current", () => {
    const l = latestWins();
    const a = l.begin();
    const b = l.begin();
    expect(a.current()).toBe(false);
    expect(b.current()).toBe(true);
    l.invalidate();
    expect(b.current()).toBe(false);
  });
});

describe("search action", () => {
  beforeEach(async () => {
    hopperMock.reset();
    resetStore();
    A.initStore();
    await vi.waitFor(() => expect(store.getState().ready).toBe(true));
  });
  afterEach(() => {
    vi.restoreAllMocks();
    vi.useRealTimers();
    resetStore();
  });

  it("debounces typing into a single backend call", async () => {
    const spy = vi.spyOn(ipc, "invoke");
    for (const q of ["u", "us", "use", "user"]) A.search({ query: q });
    expect(store.getState().search.loading).toBe(true);
    await vi.waitFor(() => expect(store.getState().search.result).not.toBeNull());
    expect(spy.mock.calls.filter((c) => c[0] === "search_routes")).toHaveLength(1);
    expect(store.getState().search.loading).toBe(false);
  });

  it("latest wins: a slow earlier answer is dropped", async () => {
    const real = ipc.invoke;
    const spy = vi.spyOn(ipc, "invoke").mockImplementation(async (cmd, args) => {
      if (cmd === "search_routes" && (args as { query: string }).query === "slow") {
        await new Promise((r) => setTimeout(r, 80));
      }
      return real(cmd, args as never);
    });
    A.search({ query: "slow" });
    const first = A.runSearch();
    A.search({ query: "roles" });
    await A.runSearch();
    await first;
    expect(spy).toHaveBeenCalled();
    const hits = store.getState().search.result!.hits;
    expect(hits[0].path).toContain("roles");
    expect(store.getState().search.query).toBe("roles");
  });

  it("an empty query clears the result without calling the backend", async () => {
    A.search({ query: "user" });
    await A.runSearch();
    expect(store.getState().search.result).not.toBeNull();
    const spy = vi.spyOn(ipc, "invoke");
    A.clearSearch();
    expect(store.getState().search.result).toBeNull();
    expect(spy).not.toHaveBeenCalled();
  });

  it("tag and method filters narrow results but facets stay", async () => {
    A.search({ query: "user" });
    await A.runSearch();
    const all = store.getState().search.result!;
    expect(all.tags.length).toBeGreaterThan(3);
    A.setSearchTag("UserRoles");
    await A.runSearch();
    const narrowed = store.getState().search.result!;
    expect(narrowed.total).toBeLessThan(all.total);
    expect(narrowed.tags.length).toBe(all.tags.length);
  });
});
