import { useCallback, useEffect, useState } from "react";
import { toHopperError, type GrantDto } from "../../ipc";
import { openSheet, setAgentAccess, toast, useStore } from "../../store";
import { Busy, Button, Chip } from "../../ui";
import { listGrants } from "../env/api";
import { Note } from "../env/controls";

export function GrantsTab() {
  const tree = useStore((s) => s.tree);
  const [grants, setGrants] = useState<GrantDto[] | null>(null);
  const [picked, setPicked] = useState<Set<string>>(new Set());

  const load = useCallback(async () => {
    try {
      const list = await listGrants();
      setGrants(list);
      setPicked((p) => new Set([...p].filter((id) => list.some((g) => g.requestId === id))));
    } catch (e) {
      toast(toHopperError(e).message, "error");
      setGrants((g) => g ?? []);
    }
  }, []);

  useEffect(() => void load(), [load, tree]);

  const revoke = async (ids: string[]) => {
    await setAgentAccess(ids, "ask");
    await load();
  };
  const toggle = (id: string) => setPicked((p) => { const n = new Set(p); if (!n.delete(id)) n.add(id); return n; });

  return (
    <div className="ag-grants">
      <Note>
        <strong>Ask</strong>: the agent must ask you before it sends this request. <strong>Auto</strong>: it may send it without asking. <strong>Blocked</strong>: it can never send it. Local environments are always allowed; production only ever runs requests you set to Auto.
      </Note>
      {grants === null ? <Busy /> : grants.length === 0 ? <p className="sh-empty">No request is set to Auto or Blocked. Every agent send will ask.</p> : (
        <>
          <div className="sh-row-actions">
            <label className="ag-check">
              <input type="checkbox" checked={picked.size === grants.length} onChange={(e) => setPicked(e.target.checked ? new Set(grants.map((g) => g.requestId)) : new Set())} />
              <span>Select all</span>
            </label>
            <span className="sh-grow" />
            <Button size="sm" disabled={picked.size === 0} onClick={() => void revoke([...picked])}>Revoke selected ({picked.size})</Button>
          </div>
          <ul className="ag-list">
            {grants.map((g) => (
              <li key={g.requestId} className="ag-grant">
                <input type="checkbox" aria-label={`Select ${g.requestName}`} checked={picked.has(g.requestId)} onChange={() => toggle(g.requestId)} />
                <span className="ag-grant-name">{g.requestName}</span>
                <Chip tone={g.access === "auto" ? "ok" : g.access === "blocked" ? "err" : "neutral"}>{g.access}</Chip>
                {g.suspended && <Chip tone="warn" tip="The agent edited this request, so the grant is paused">changed by agent</Chip>}
                <Button size="sm" onClick={() => void revoke([g.requestId])} aria-label={`Set ${g.requestName} back to Ask`}>{g.suspended ? "Reset to Ask" : "Revoke"}</Button>
              </li>
            ))}
          </ul>
        </>
      )}
      <Button size="sm" variant="ghost" icon="sliders" onClick={() => openSheet("environments")}>Per-environment rules</Button>
    </div>
  );
}
