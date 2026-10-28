import { Sprite } from "./Sprite";

export type MascotState = "sleeping" | "sending" | "agent" | "approval" | "error";

/** Hopper's mascot: a pixel hopper. It sleeps, jiggles while a request drops through, and shows a "!" when an approval waits. */
export function Mascot({ state, size = 40 }: { state: MascotState; size?: number }) {
  return (
    <span className={`mascot mascot--${state}`} style={{ width: size, height: size }} role="img" aria-label={`Hopper: ${state}`}>
      <Sprite name="hopper" size={size * 0.8} className="mascot-hop" />
      <Sprite name="gold" size={size * 0.36} className="mascot-item" />
      <i className="mascot-z" aria-hidden="true">z</i>
      <i className="mascot-bang" aria-hidden="true">!</i>
    </span>
  );
}
