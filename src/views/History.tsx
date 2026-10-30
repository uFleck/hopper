import { useEffect, useMemo, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import type { HistoryRow } from "../ipc";
import { clearAllHistory, deleteRun, labelRun, openRun, refreshHistory, saveRequest, useStore } from "../store";
import { Chip, Icon, cx, confirm, statusTone, useContextMenu } from "../ui";
import type { MenuItem } from "./find/Menu";
import { highlightRanges, matchesHistory } from "./find/historyFilter";
import "../styles/history.css";

/** Easter eggs on a label: a name tag that says Dinnerbone or Grumm flips the row, jeb_ cycles the colour. */
function eggClass(label: string): string | undefined {
  const l = label.trim().toLowerCase();
  if (l === "dinnerbone" || l === "grumm") return "hist-egg-flip";
  if (l === "jeb_") return "hist-egg-jeb";
  return undefined;
}

type Filter = "all" | "named" | "failed" | "unsaved";

const FILTERS: [Filter, string][] = [
  ["all", "All"],
  ["named", "Named"],
  ["failed", "Failed"],
  ["unsaved", "Unsaved"],
];

const DAY_MS = 86400000;
const isFailed = (r: HistoryRow) => r.status === 0 || r.status >= 400;
const isAgent = (r: HistoryRow) => r.via === "mcp";
const passes = (r: HistoryRow, f: Filter) =>
  f === "all" || (f === "named" ? !!r.label : f === "failed" ? isFailed(r) : !r.saved);
const keyOf = (r: HistoryRow) => `${r.requestId}/${r.id}`;

/** Today, Yesterday, Last week, then the month. */
function dayOf(sentAt: string, now: Date): string {
  const sent = new Date(sentAt);
  const midnight = (d: Date) => new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
  const days = Math.round((midnight(now) - midnight(sent)) / DAY_MS);
  if (days <= 0) return "Today";
  if (days === 1) return "Yesterday";
  if (days < 14) return "Last week";
  return sent.toLocaleDateString(undefined, { month: "long", year: "numeric" });
}

/** Minutes for the last hour, then the clock time today, then the date. */
function whenOf(sentAt: string, now: Date): string {
  const sent = new Date(sentAt);
  const mins = Math.round((now.getTime() - sent.getTime()) / 60000);
  if (mins < 60) return mins < 1 ? "now" : `${mins} min`;
  if (dayOf(sentAt, now) === "Today") return sent.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
  return sent.toLocaleDateString(undefined, { month: "short", day: "numeric" });
}

/** Text with the query words wrapped in `<mark>`. */
function Hl({ text, q }: { text: string; q: string }) {
  const parts: ReactNode[] = [];
  let last = 0;
  for (const [a, b] of highlightRanges(text, q)) {
    if (a > last) parts.push(text.slice(last, a));
    parts.push(<mark key={a}>{text.slice(a, b)}</mark>);
    last = b;
  }
  if (last < text.length) parts.push(text.slice(last));
  return <>{parts}</>;
}

interface RowProps {
  row: HistoryRow;
  q: string;
  now: Date;
  selected: boolean;
  editing: boolean;
  onSelect: () => void;
  onEdit: (on: boolean) => void;
}

function Row({ row, q, now, selected, editing, onSelect, onEdit }: RowProps) {
  const current = row.label ?? "";
  const [draft, setDraft] = useState(row.label ?? row.name);
  const skipBlur = useRef(false);
  const openTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  useEffect(() => () => clearTimeout(openTimer.current), []);

  // A double-click renames, so a single click waits a moment to see whether a second one follows.
  const clickOpen = () => {
    onSelect();
    clearTimeout(openTimer.current);
    openTimer.current = setTimeout(() => void openRun(row), 220);
  };
  const items: MenuItem[] = [
    { key: "rename", label: row.label ? "Rename label" : "Add label", icon: "tag", trailing: <kbd className="ui-kbd">F2</kbd>, onSelect: () => startEdit() },
    ...(row.label ? [{ key: "unlabel", label: "Remove label", icon: "x-circle", onSelect: () => void labelRun(row.requestId, row.id, null) }] : []),
    { key: "open", label: row.saved ? "Open request" : "Open as draft", icon: "arrow-square-out", separated: true, onSelect: () => void openRun(row) },
    ...(!row.saved && row.hasRequest
      ? [{ key: "save", label: "Save as request", icon: "floppy-disk", onSelect: () => void openRun(row).then(() => saveRequest()) }]
      : []),
    { key: "delete", label: "Delete from history", icon: "trash", tone: "danger" as const, separated: true, onSelect: () => deleteRun(row.requestId, row.id) },
  ];
  const { onContextMenu, menu } = useContextMenu(items, "Run");

  const startEdit = () => {
    skipBlur.current = false;
    setDraft(row.label ?? row.name);
    onEdit(true);
  };
  const save = () => {
    if (skipBlur.current) return;
    skipBlur.current = true;
    onEdit(false);
    const next = draft.trim();
    if (next === current || (!current && next === row.name)) return;
    void labelRun(row.requestId, row.id, next || null);
  };
  const cancel = () => {
    skipBlur.current = true;
    onEdit(false);
  };

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.target !== e.currentTarget) return;
    if (e.key === "Enter") void openRun(row);
    else if (e.key === "F2") startEdit();
    else if (e.key === "Delete") deleteRun(row.requestId, row.id);
  };

  const label = row.label ? (
    <span className={cx("hist-lbl", eggClass(row.label))} data-tip="Name-tagged. This run will not despawn.">
      <Icon name="tag" />
      <span className="hist-rn"><Hl text={row.label} q={q} /></span>
    </span>
  ) : (
    <span className="hist-rn"><Hl text={row.name} q={q} /></span>
  );

  return (
    <div className={cx("hist-row", selected && "sel")} role="listitem" tabIndex={0} onClick={clickOpen} onKeyDown={onKeyDown} onContextMenu={onContextMenu}>
      <Chip method={row.method} />
      <div className="hist-mid">
        <div className="hist-title" onDoubleClick={(e) => {
            e.stopPropagation();
            clearTimeout(openTimer.current);
            startEdit();
          }}>
          {editing ? (
            <input
              className="hist-edit"
              value={draft}
              autoFocus
              aria-label="Label"
              onFocus={(e) => e.currentTarget.select()}
              onChange={(e) => setDraft(e.target.value)}
              onClick={(e) => e.stopPropagation()}
              onKeyDown={(e) => {
                e.stopPropagation();
                if (e.key === "Enter") save();
                else if (e.key === "Escape") cancel();
              }}
              onBlur={save}
            />
          ) : (
            <>
              {label}
              {!row.saved && <span className="hist-tag">UNSAVED</span>}
              {isAgent(row) && <span className="hist-tag mcp">AGENT</span>}
            </>
          )}
        </div>
        <div className="hist-sub">
          <Hl text={row.label ? `${row.name} · ${row.url}` : row.url} q={q} />
        </div>
      </div>
      <div className="hist-end">
        <Chip tone={statusTone(row.status)}>{row.status || "ERR"}</Chip>
        <span className="hist-when">{whenOf(row.sentAt, now)}</span>
      </div>
      {!editing && (
        <button
          type="button"
          className="hist-pen"
          aria-label="Rename"
          onClick={(e) => {
            e.stopPropagation();
            startEdit();
          }}
        >
          <Icon name="pencil-simple" />
        </button>
      )}
      {menu}
    </div>
  );
}

/** The run history of the whole workspace, newest first, grouped by day. The search text comes from the find box. */
export default function History({ query }: { query: string }) {
  const rows = useStore((s) => s.historyAll);
  const [filter, setFilter] = useState<Filter>("all");
  const [agentOn, setAgentOn] = useState(false);
  const [selected, setSelected] = useState<string | null>(null);
  const [editing, setEditing] = useState<string | null>(null);
  const now = new Date();

  useEffect(() => void refreshHistory(), []);

  const counts: Record<Filter, number> = {
    all: rows.length,
    named: rows.filter((r) => !!r.label).length,
    failed: rows.filter(isFailed).length,
    unsaved: rows.filter((r) => !r.saved).length,
  };
  const agentCount = rows.filter(isAgent).length;

  const visible = useMemo(
    () =>
      rows.filter((r) => (agentOn || !isAgent(r)) && passes(r, filter) && (!query.trim() || matchesHistory(r, query))),
    [rows, agentOn, filter, query],
  );

  const groups: [string, HistoryRow[]][] = [];
  for (const r of visible) {
    const day = dayOf(r.sentAt, now);
    const last = groups[groups.length - 1];
    if (last && last[0] === day) last[1].push(r);
    else groups.push([day, [r]]);
  }

  const clearAll = async () => {
    const ok = await confirm({
      title: "Clear all history?",
      message: "Every run goes, labelled runs too. Saved requests are not affected.",
      confirmLabel: "Clear all",
      tone: "danger",
    });
    if (ok) await clearAllHistory();
  };

  if (rows.length === 0) {
    return (
      <div className="hist">
        <div className="hist-empty">
          <Icon name="clock" />
          <b>Nothing sent yet</b>
          Requests you send show up here, newest first. Give the important ones a label and Hopper keeps them for good.
        </div>
      </div>
    );
  }

  return (
    <div className="hist">
      <div className="hist-chips">
        {FILTERS.map(([f, name]) => (
          <button
            key={f}
            type="button"
            className={cx("hist-chip", filter === f && "on")}
            aria-pressed={filter === f}
            onClick={() => setFilter(f)}
          >
            {f === "named" && <Icon name="tag" />}
            {name} <b>{counts[f]}</b>
          </button>
        ))}
        <button
          type="button"
          className={cx("hist-chip", agentOn && "on")}
          aria-pressed={agentOn}
          onClick={() => setAgentOn((on) => !on)}
        >
          Agent <b>{agentCount}</b>
        </button>
      </div>

      <div className="hist-scroll">
        {visible.length === 0 && (
          <div className="hist-empty">
            <b>No matches</b>
            {query.trim() ? `Nothing in history matches “${query.trim()}”.` : "No runs match these filters."}
          </div>
        )}
        {groups.map(([day, list]) => (
          <div key={day}>
            <div className="hist-day">{day}</div>
            {list.map((r) => (
              <Row
                key={keyOf(r)}
                row={r}
                q={query.trim()}
                now={now}
                selected={selected === keyOf(r)}
                editing={editing === keyOf(r)}
                onSelect={() => setSelected(keyOf(r))}
                onEdit={(on) => setEditing(on ? keyOf(r) : null)}
              />
            ))}
          </div>
        ))}
        {editing && <div className="hist-hint"><span>Enter save</span><span>Esc cancel</span><span>empty removes the label</span><span>Name tag: keeps this run for good</span></div>}
      </div>

      <div className="hist-foot">
        <span>{rows.length} runs kept · {rows.filter((r) => r.label).length} name-tagged never despawn</span>
        <span className="hist-sp" />
        <button type="button" onClick={() => void clearAll()}>Clear all</button>
      </div>
    </div>
  );
}
