import { useMemo } from "react";
import { openRequest, useStore } from "../store";
import { Chip, statusTone } from "../ui";
import { flattenRequests } from "./find/model";
import { queryWords } from "./find/historyFilter";

/** The last requests sent, one line each, with the status of its last run. */
export default function Recent({ query = "" }: { query?: string }) {
  const recent = useStore((s) => s.recent);
  const tree = useStore((s) => s.tree);
  const where = useMemo(() => new Map(flattenRequests(tree).map((r) => [r.id, [r.collection, ...r.folder].join(" / ")])), [tree]);
  const words = queryWords(query);
  const shown = words.length
    ? recent.filter((r) => {
        const hay = `${r.name} ${r.method} ${r.status ?? ""} ${where.get(r.id) ?? ""}`.toLowerCase();
        return words.every((w) => hay.includes(w));
      })
    : recent;
  if (recent.length === 0) return <p className="recent-empty">Nothing sent yet.</p>;
  if (shown.length === 0) return <p className="recent-empty">No matches.</p>;

  return (
    <div className="recent-list" role="list" aria-label="Recent">
      {shown.map((r) => (
        <button key={r.id} type="button" role="listitem" className="recent-row" onClick={() => void openRequest(r.id)} data-tip={`${r.name} · ${where.get(r.id) ?? ""}`}>
          <Chip method={r.method} />
          <span className="recent-row-name">{r.name}</span>
          <span className="recent-row-where">{where.get(r.id)}</span>
          {r.status != null && <Chip tone={statusTone(r.status)} className="recent-status">{r.status}</Chip>}
        </button>
      ))}
    </div>
  );
}
