import { describe, expect, it } from "vitest";
import type { HistoryRow } from "../../ipc";
import { highlightRanges, matchesHistory } from "./historyFilter";

const row = (over: Partial<HistoryRow> = {}): HistoryRow =>
  ({
    id: "r1",
    requestId: "q1",
    env: "dev",
    via: "ui",
    sentAt: "2026-10-30T10:00:00Z",
    method: "POST",
    url: "https://api.test/api/invoices",
    status: 422,
    totalMs: 212,
    sizeBytes: 10,
    hasRequest: true,
    name: "Create invoice",
    saved: true,
    ...over,
  }) as HistoryRow;

describe("matchesHistory", () => {
  it("matches the label", () => {
    expect(matchesHistory(row({ label: "VAT bug repro" }), "vat")).toBe(true);
    expect(matchesHistory(row({ label: "Login" }), "vat")).toBe(false);
  });

  it("matches the request name", () => {
    expect(matchesHistory(row(), "INVOICE")).toBe(true);
    expect(matchesHistory(row(), "orders")).toBe(false);
  });

  it("matches the URL", () => {
    expect(matchesHistory(row(), "api.test")).toBe(true);
    expect(matchesHistory(row(), "/api/users")).toBe(false);
  });

  it("matches the method", () => {
    expect(matchesHistory(row(), "post")).toBe(true);
    expect(matchesHistory(row(), "delete")).toBe(false);
  });

  it("matches the status", () => {
    expect(matchesHistory(row(), "422")).toBe(true);
    expect(matchesHistory(row(), "500")).toBe(false);
  });

  it("requires every word to match, each word anywhere in the fields", () => {
    expect(matchesHistory(row({ label: "VAT bug repro" }), "vat 422")).toBe(true);
    expect(matchesHistory(row({ label: "VAT bug repro" }), "vat 500")).toBe(false);
    expect(matchesHistory(row(), "  post   invoice  ")).toBe(true);
  });

  it("matches everything for an empty query", () => {
    expect(matchesHistory(row(), "   ")).toBe(true);
  });
});

describe("highlightRanges", () => {
  it("returns every occurrence of each word, case-insensitively", () => {
    expect(highlightRanges("Invoice, invoices", "invoice")).toEqual([
      [0, 7],
      [9, 16],
    ]);
  });

  it("merges overlapping word matches", () => {
    expect(highlightRanges("abcdef", "abc bcd")).toEqual([[0, 4]]);
  });

  it("returns nothing for an empty query", () => {
    expect(highlightRanges("anything", "  ")).toEqual([]);
  });
});
