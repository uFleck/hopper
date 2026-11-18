import { describe, expect, it } from "vitest";
import { MODES, MODE_ORDER, accessOf, modeOf } from "./modes";

describe("agent modes", () => {
  it("round-trips every mode through access and scope", () => {
    for (const mode of MODE_ORDER) {
      const { access, scope } = accessOf(mode);
      expect(modeOf(access, scope)).toBe(mode);
    }
  });

  it("ignores a stale scope on Ask and Blocked", () => {
    expect(modeOf("ask", "any")).toBe("ask");
    expect(modeOf("blocked", "params")).toBe("blocked");
    expect(accessOf("blocked").scope).toBe("exact");
  });

  it("warns only for the opt-in modes", () => {
    expect(MODE_ORDER.filter((m) => MODES[m].risk)).toEqual(["auto_params", "auto_any"]);
  });
});
