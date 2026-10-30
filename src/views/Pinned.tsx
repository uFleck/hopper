import { useMemo } from "react";
import { openRequest, useStore } from "../store";
import { usePins } from "../store/pins";
import { cx, methodClass } from "../ui";
import { flattenRequests, type FlatRequest } from "./find/model";

/** The lamp colour for a last status: 2xx, 3xx, 4xx, 5xx (and network error, 0); none when unknown. */
function lampClass(status: number | null | undefined): string {
  if (status == null) return "";
  if (status === 0 || status >= 500) return "is-5xx";
  if (status >= 400) return "is-4xx";
  if (status >= 300) return "is-3xx";
  if (status >= 200) return "is-2xx";
  return "";
}

/** The pinned saved requests as one-line chips; each bulb shows the last result of that request. */
export default function Pinned() {
  const pins = usePins();
  const tree = useStore((s) => s.tree);
  const recent = useStore((s) => s.recent);
  const saved = useMemo(() => flattenRequests(tree), [tree]);
  const savedById = useMemo(() => new Map<string, FlatRequest>(saved.map((r) => [r.id, r])), [saved]);
  const pinned = pins.flatMap((id) => {
    const r = savedById.get(id);
    return r ? [r] : [];
  });
  if (pinned.length === 0) return null;

  return (
    <section className="pinned" aria-label="Pinned">
      <h2 className="pinned-title">Pinned</h2>
      <div className="lamps">
        {pinned.map((r) => {
          const status = recent.find((x) => x.id === r.id)?.status;
          return (
            <button key={r.id} type="button" className="lamp" onClick={() => void openRequest(r.id)} data-tip={`${r.name} · ${r.collection}`}>
              <span className={cx("lamp-m", methodClass(r.method))}>{r.method}</span>
              <span className="lamp-name">{r.name}</span>
              <span className={cx("lamp-bulb", lampClass(status))} data-tip={status != null ? `Last result ${status}` : "Not sent yet"} aria-hidden="true" />
            </button>
          );
        })}
      </div>
    </section>
  );
}
