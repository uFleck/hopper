import { useCallback, useEffect, useState } from "react";
import { toHopperError, type GrantDto } from "../../ipc";
import { openSheet, setAgentAccess, toast, useStore } from "../../store";
import { AGENT_MODES, AGENT_MODE_ORDER, agentMode, type AgentMode } from "../../agentModes";
import { Busy, Button, Chip, Select } from "../../ui";
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
  const switchMode = async (id: string, mode: AgentMode) => {
    const m = AGENT_MODES[mode];
    await setAgentAccess([id], m.access, m.scope);
    await load();
  };
  const toggle = (id: string) => setPicked((p) => { const n = new Set(p); if (!n.delete(id)) n.add(id); return n; });

  return (
    <div className="ag-grants">
      <Note>
        <strong>Ask</strong>: the agent must ask you before it sends this request. <strong>Auto</strong>: it may send it as it is now without asking; any edit asks again. <strong>Auto: parameters may change</strong> also lets it change path and query values. <strong>Auto: trusted</strong> lets it change anything but the host and environment. <strong>Blocked</strong>: it can never send it. Local environments are always allowed; production only ever runs requests you set to Auto.
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
              const mode = agentMode(g.access, g.scope);
              const warning = AGENT_MODES[mode].warning;
              return (
                <li key={g.requestId} className="ag-grant">
                  <input type="checkbox" aria-label={`Select ${g.requestName}`} checked={picked.has(g.requestId)} onChange={() => toggle(g.requestId)} />
                  <span className="ag-grant-name">{g.requestName}</span>
                  <Select className="ag-grant-mode" aria-label={`Agent access for ${g.requestName}`} value={mode} onChange={(e) => void switchMode(g.requestId, e.target.value as AgentMode)}>
                    {AGENT_MODE_ORDER.map((m) => <option key={m} value={m}>{AGENT_MODES[m].label}</option>)}
                  </Select>
                  {g.suspended && <Chip tone="warn" tip="The agent edited this request, so the grant is paused">changed by agent</Chip>}
                  <Button size="sm" onClick={() => void revoke([g.requestId])} aria-label={`Set ${g.requestName} back to Ask`}>{g.suspended ? "Reset to Ask" : "Revoke"}</Button>
                  {warning && !g.suspended && <p className="ag-grant-warning">{warning}</p>}
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
