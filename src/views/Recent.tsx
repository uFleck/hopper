import { openRequest, useStore } from "../store";
import { Chip, statusTone } from "../ui";

const RECENT_SHOWN = 5;

/** The last requests sent, each with the status of its last run. */
export default function Recent() {
  const recent = useStore((s) => s.recent);
  return (
    <section className="recent" aria-label="Recent">
      <h2 className="recent-title">Recent</h2>
      {recent.length === 0 ? (
        <p className="recent-empty">Nothing sent yet.</p>
      ) : (
        <div className="recent-list">
          {recent.slice(0, RECENT_SHOWN).map((r) => (
            <button key={r.id} type="button" className="recent-row" onClick={() => void openRequest(r.id)} title={r.name}>
              <Chip method={r.method} />
              <span className="recent-row-name">{r.name}</span>
              {r.status != null && <Chip tone={statusTone(r.status)} className="recent-status">{r.status}</Chip>}
            </button>
          ))}
        </div>
      )}
    </section>
  );
}
