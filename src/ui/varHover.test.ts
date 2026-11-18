import { describe, expect, it } from "vitest";
import { variableTokenIn } from "./Editor";
import { placeCard } from "./useHoverCard";

describe("variableTokenIn", () => {
  it("finds the token covering a character", () => {
    expect(variableTokenIn("a {{host}}/b", 4)).toEqual({ name: "host", from: 2, to: 10 });
  });

  it("does not count the character after a token as part of it", () => {
    expect(variableTokenIn("{{host}} x", 8)).toBeNull();
  });

  it("gives the second of two adjacent tokens at their shared boundary", () => {
    expect(variableTokenIn("{{a}}{{b}}", 5)?.name).toBe("b");
  });

  it("ignores an empty name", () => {
    expect(variableTokenIn("{{}}", 1)).toBeNull();
  });
});

describe("placeCard", () => {
  const vw = 400;
  const vh = 300;

  it("hangs below the anchor when it fits", () => {
    expect(placeCard({ left: 20, top: 50, above: 30 }, 200, 100, vw, vh)).toEqual({ left: 20, top: 50, side: "below" });
  });

  it("flips above when there is no room below", () => {
    expect(placeCard({ left: 20, top: 250, above: 230 }, 200, 100, vw, vh)).toEqual({ left: 20, top: 130, side: "above" });
  });

  it("slides in from the right edge", () => {
    expect(placeCard({ left: 350, top: 50, above: 30 }, 200, 100, vw, vh).left).toBe(vw - 200 - 8);
  });

  it("stays fully on screen when neither side has room", () => {
    const below = placeCard({ left: 20, top: 120, above: 100 }, 200, 250, vw, vh);
    expect(below).toEqual({ left: 20, top: vh - 250 - 8, side: "below" });
    const above = placeCard({ left: 20, top: 200, above: 180 }, 200, 250, vw, vh);
    expect(above).toEqual({ left: 20, top: 8, side: "above" });
  });
});
