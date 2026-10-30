import { describe, expect, it } from "vitest";
import { clampToViewport } from "./ContextMenu";
import { placeTooltip } from "./Tooltip";

describe("clampToViewport", () => {
  it("keeps a box inside the viewport with an 8 px margin", () => {
    expect(clampToViewport(100, 240, 1000)).toBe(100);
    expect(clampToViewport(900, 240, 1000)).toBe(752);
    expect(clampToViewport(-20, 240, 1000)).toBe(8);
  });

  it("pins to the margin when the box is larger than the viewport", () => {
    expect(clampToViewport(50, 2000, 400)).toBe(8);
  });
});

describe("placeTooltip", () => {
  const viewport = { width: 1000, height: 600 };
  const size = { width: 80, height: 24 };

  it("centres under the anchor with a 6 px gap", () => {
    expect(placeTooltip({ left: 100, top: 50, width: 40, height: 20 }, size, viewport)).toEqual({ left: 80, top: 76 });
  });

  it("flips above the anchor when there is no room below", () => {
    expect(placeTooltip({ left: 100, top: 580, width: 40, height: 20 }, size, viewport)).toEqual({ left: 80, top: 550 });
  });

  it("stays inside the left edge", () => {
    expect(placeTooltip({ left: 0, top: 50, width: 10, height: 20 }, size, viewport).left).toBe(8);
  });
});
