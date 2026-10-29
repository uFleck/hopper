import { useMemo } from "react";
import { openRequest, useStore } from "../store";
import { Chip, statusTone } from "../ui";
import { flattenRequests } from "./find/model";

/** The last requests sent, one line each, with the status of its last run. */
export default function Recent() {
  const recent = useStore((s) => s.recent);
  const tree = useStore((s) => s.tree);
  const where = useMemo(() => new Map(flattenRequests(tree).map((r) => [r.id, [r.collection, ...r.folder].join(" / ")])), [tree]);
  if (recent.length === 0) return <p className="recent-empty">Nothing sent yet.</p>;

  return (
    <div className="recent-list" role="list" aria-label="Recent">
      {recent.map((r) => (
        <button key={r.id} type="button" role="listitem" className="recent-row" onClick={() => void openRequest(r.id)} title={`${r.name} · ${where.get(r.id) ?? ""}`}>
          <Chip method={r.method} />
          <span className="recent-row-name">{r.name}</span>
          <span className="recent-row-where">{where.get(r.id)}</span>
          {r.status != null && <Chip tone={statusTone(r.status)} className="recent-status">{r.status}</Chip>}
        </button>
      ))}
    </div>
  );
}
