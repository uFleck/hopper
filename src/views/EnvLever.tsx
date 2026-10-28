import type { CSSProperties } from "react";
import { selectActiveEnv, setActiveEnv, useStore } from "../store";
import { cx, envClass } from "../ui";

/** The environment lever: the four fixed stages are notches, the thumb sits on the active one, and the whole row stays in view. */
export default function EnvLever() {
  const environments = useStore((s) => s.environments);
  const active = useStore(selectActiveEnv);
  if (environments.length === 0) return null;

  const index = environments.findIndex((e) => e.active);
  const style = { "--n": environments.length, "--i": Math.max(index, 0) } as CSSProperties;

  return (
    <div className="lever-wrap">
      <div className="lever" role="radiogroup" aria-label="Environment" data-none={!active || undefined}>
        <div className="lever-track" style={style}>
          {active && <span className={cx("lever-thumb", envClass(active))} aria-hidden="true" />}
          {environments.map((e) => {
            const gaps = e.applications.flatMap((a) => a.secrets).filter((x) => x.status === "missing").length;
            return (
              <button
                key={e.stage}
                type="button"
                role="radio"
                aria-checked={e.active}
                className={cx("lever-notch", envClass(e.stage))}
                title={gaps > 0 ? `${e.stage}: ${gaps} secret${gaps === 1 ? "" : "s"} missing` : e.stage}
                onClick={() => void setActiveEnv(e.stage)}
              >
                <i aria-hidden="true" />
                <span className="lever-name">{e.stage}</span>
                {gaps > 0 && <span className="hdr-warn-dot" aria-label={`${gaps} secrets missing`} />}
              </button>
            );
          })}
        </div>
      </div>
      {active === "production" && <p className="lever-guard" role="status">Production. Destructive requests need you to arm them first.</p>}
    </div>
  );
}
