import { useEffect, useMemo, useState } from "react";
import { toHopperError, type StageDto, type Method, type PatternRule } from "../../ipc";
import { refreshEnvironments, toast } from "../../store";
import { Button, IconButton, Select, TextInput } from "../../ui";
import { saveAgentRules } from "./api";
import { Note, Switch } from "./controls";
import { describeRule, sameRules, validateRule } from "./draft";

const METHODS: Method[] = ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];

/** User-local rules for what the agent may send in one environment. Saved on their own, not with the environment file. */
export function AgentRules({ env }: { env: StageDto }) {
  const saved = env.agentRules;
  const savedKey = JSON.stringify(saved);
  const [rules, setRules] = useState(saved);
  const [busy, setBusy] = useState(false);

  useEffect(() => setRules(JSON.parse(savedKey)), [savedKey, env.stage]);

  const local = env.stage === "local";
  const production = env.stage === "production";
  const errors = useMemo(() => rules.patterns.map((p: PatternRule) => validateRule(p, env.stage)), [rules.patterns, env.stage]);
  const dirty = !sameRules(rules, saved);
  const setPattern = (i: number, patch: Partial<PatternRule>) => setRules({ ...rules, patterns: rules.patterns.map((p, j) => (j === i ? { ...p, ...patch } : p)) });

  const save = async () => {
    setBusy(true);
    try {
      await saveAgentRules(env.stage, rules.deny, rules.safeReads, rules.patterns.map((p: PatternRule) => ({ ...p, host: p.host.trim(), path: p.path.trim() })));
      await refreshEnvironments();
      toast("Agent rules saved", "success");
    } catch (e) {
      toast(toHopperError(e).message, "error");
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="sh-rules">
      <p className="sh-help">
        These rules are yours alone and are never written to the shared environment file. They decide what the agent may send in <strong>{env.stage}</strong> without asking.
      </p>
      {local && <Note>Local is always allowed. The agent can send any request here without asking, so these rules are not used.</Note>}
      {production && <Note tone="danger">Production can never be allowed wholesale. Open it up one request at a time with Auto on the request itself; rules here only cover narrow patterns.</Note>}

      <Switch checked={rules.deny} disabled={local} onChange={(deny) => setRules({ ...rules, deny })} label="Deny the agent here" hint="The agent cannot send anything in this environment, not even requests set to Auto." />
      <Switch checked={rules.safeReads} disabled={local || rules.deny} onChange={(safeReads) => setRules({ ...rules, safeReads })} label="Allow safe reads" hint={production ? "GET, HEAD and OPTIONS to allowed hosts run without asking. Off by default for production: think twice." : "GET, HEAD and OPTIONS to allowed hosts run without asking."} />

      <h4 className="sh-subtitle">Pattern rules</h4>
      <p className="sh-help">A rule lets the agent send a request it built itself when the method, host and path all match. In the path, <code>*</code> matches one segment and <code>**</code> matches several. Example: GET on <code>api.example.com</code> with <code>/users/*</code>.</p>

      {rules.patterns.length === 0 && <p className="sh-empty">No pattern rules.</p>}
      <div className="sh-table-scroll">
        {rules.patterns.map((p, i) => (
          <div key={i} className="sh-rule">
            <div className="sh-rule-row">
              <Select aria-label="Method" disabled={local || rules.deny} value={p.method ?? ""} onChange={(e) => setPattern(i, { method: (e.target.value || undefined) as Method | undefined })}>
                <option value="">Any</option>
                {METHODS.map((m) => <option key={m} value={m}>{m}</option>)}
              </Select>
              <TextInput mono aria-label="Host" placeholder="api.example.com" disabled={local || rules.deny} invalid={!!errors[i]} value={p.host} onChange={(e) => setPattern(i, { host: e.target.value })} />
              <TextInput mono aria-label="Path glob" placeholder="/users/*" disabled={local || rules.deny} invalid={!!errors[i]} value={p.path} onChange={(e) => setPattern(i, { path: e.target.value })} />
              <IconButton icon="trash" label="Remove rule" size="sm" tone="danger" disabled={local} onClick={() => setRules({ ...rules, patterns: rules.patterns.filter((_, j) => j !== i) })} />
            </div>
            {errors[i] ? <p className="ui-field-error" role="alert">{errors[i]}</p> : <p className="sh-rule-says">{describeRule(p)}</p>}
          </div>
        ))}
      </div>
      <div className="sh-row-actions">
        <Button size="sm" icon="plus" disabled={local || rules.deny} onClick={() => setRules({ ...rules, patterns: [...rules.patterns, { method: "GET", host: "", path: "/" }] })}>Add rule</Button>
        <span className="sh-grow" />
        {dirty && <Button size="sm" variant="ghost" onClick={() => setRules(saved)}>Discard</Button>}
        <Button size="sm" variant="primary" loading={busy} disabled={!dirty || errors.some(Boolean) || local} onClick={save}>Save rules</Button>
      </div>
    </div>
  );
}
