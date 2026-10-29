import { selectActiveEnv, setActiveEnv, useStore } from "../store";
import { cx, envClass } from "../ui";
import { Menu, type MenuItem } from "./find/Menu";

/** The environment select in the header: the four fixed stages, the active one marked, a dot for missing secrets. */
export function EnvPicker() {
  const environments = useStore((s) => s.environments);
  const active = useStore(selectActiveEnv);
  if (environments.length === 0) return null;

  const items: MenuItem[] = environments.map((e) => {
    const gaps = e.applications.flatMap((a) => a.secrets).filter((x) => x.status === "missing").length;
    return {
      key: e.stage,
      label: (
        <span className={cx("env-opt", envClass(e.stage))}>
          <i aria-hidden="true" />
          {e.stage}
        </span>
      ),
      trailing: gaps > 0 ? <span className="hdr-warn-dot" role="img" aria-label={`${gaps} secrets missing`} title={`${gaps} secret${gaps === 1 ? "" : "s"} missing`} /> : undefined,
      selected: e.active,
      onSelect: () => void setActiveEnv(e.stage),
    };
  });

  return (
    <Menu
      label="Environment"
      items={items}
      trigger={(props) => (
        <button {...props} type="button" className={cx("env-pill", active ? envClass(active) : "is-none")} aria-label={`Environment: ${active ?? "none"}`} title="Environment">
          <i aria-hidden="true" />
          <span>{active ?? "no env"}</span>
          <i className="ph ph-caret-down env-pill-caret" aria-hidden="true" />
        </button>
      )}
    />
  );
}

/** The production warning, under the header only while production is active. */
export function EnvGuard() {
  const active = useStore(selectActiveEnv);
  if (active !== "production") return null;
  return <p className="lever-guard" role="status">Production. Destructive requests need you to arm them first.</p>;
}
