import type { ActivityEntry } from "../../ipc";
import { useStore } from "../../store";
import { Chip, type Tone } from "../../ui";

const OUTCOME: Record<ActivityEntry["outcome"], { tone: Tone; label: string }> = {
  ok: { tone: "ok", label: "ok" },
  denied: { tone: "err", label: "denied" },
  not_approved: { tone: "warn", label: "not approved" },
  error: { tone: "err", label: "error" },
};

const time = (iso: string) => new Date(iso).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" });

export function ActivityTab() {
  const activity = useStore((s) => s.activity);
  if (activity.length === 0) return <p className="sh-empty">Nothing yet. When an agent searches or sends through Hopper, it shows up here live.</p>;
  return (
    <ol className="ag-feed" aria-label="Agent activity" aria-live="polite">
      {activity.map((a) => (
        <li key={a.id} className="ag-item">
          <div className="ag-item-top">
            <time className="ag-time" dateTime={a.at}>{time(a.at)}</time>
            <code className="ag-tool">{a.tool}</code>
            <Chip tone={OUTCOME[a.outcome].tone}>{OUTCOME[a.outcome].label}</Chip>
          </div>
          <div className="ag-target">{a.target}</div>
          <div className="ag-sub">
            <span>{a.client}</span>
            {a.allowedBy && <span>allowed by: {a.allowedBy}</span>}
            {a.detail && <span>{a.detail}</span>}
          </div>
        </li>
      ))}
    </ol>
  );
}
