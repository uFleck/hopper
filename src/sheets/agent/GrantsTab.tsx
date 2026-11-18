import { useCallback, useEffect, useState } from "react";
import { toHopperError, type GrantDto } from "../../ipc";
import { openSheet, setAgentAccess, toast, useStore } from "../../store";
import { Busy, Button, Chip, Select } from "../../ui";
import { listGrants } from "../env/api";
import { Note } from "../env/controls";
import { MODES, MODE_ORDER, accessOf, modeOf, type AgentMode } from "./modes";

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
  const switchMode = async (id: string, mode: AgentMode) => {
    const next = accessOf(mode);
    await setAgentAccess([id], next.access, next.scope);
    await load();
  };
  const toggle = (id: string) => setPicked((p) => { const n = new Set(p); if (!n.delete(id)) n.add(id); return n; });

  return (
    <div className="ag-grants">
      <Note>
        <strong>Ask</strong>: the agent must ask you before it sends this request. <strong>Auto</strong>: it may send it as saved without asking; any edit pauses that. <strong>Auto: parameters may change</strong> also lets it change path and query values. <strong>Auto: trusted, any edit</strong> lets it rewrite the request, still only to the same host. <strong>Blocked</strong>: it can never send it. Local environments are always allowed; production only ever runs requests you set to Auto.
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
            {grants.map((g) => {
              // A paused grant acts as Ask until re-allowed: picking its mode again re-allows it.
              const mode = g.suspended ? "ask" : modeOf(g.access, g.scope);
              const risk = MODES[mode].risk;
              return (
                <li key={g.requestId} className="ag-grant">
                  <input type="checkbox" aria-label={`Select ${g.requestName}`} checked={picked.has(g.requestId)} onChange={() => toggle(g.requestId)} />
                  <span className="ag-grant-name">{g.requestName}</span>
                  {g.suspended && <Chip tone="warn" tip="The agent edited this request, so the grant is paused">changed by agent</Chip>}
                  <Select aria-label={`Agent access for ${g.requestName}`} value={mode} onChange={(e) => void switchMode(g.requestId, e.target.value as AgentMode)}>
                    {MODE_ORDER.map((k) => <option key={k} value={k}>{MODES[k].label}</option>)}
                  </Select>
                  <Button size="sm" onClick={() => void revoke([g.requestId])} aria-label={`Set ${g.requestName} back to Ask`}>{g.suspended ? "Reset to Ask" : "Revoke"}</Button>
                  {risk && <p className="ag-grant-risk">{risk}</p>}
                </li>
              );
            })}
          </ul>
        </>
      )}
      <Button size="sm" variant="ghost" icon="sliders" onClick={() => openSheet("environments")}>Per-environment rules</Button>
    </div>
  );
}
