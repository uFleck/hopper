import { useState, type ReactNode } from "react";
import type { ApplicationEnvDto, SecretStatus, StageDto } from "../../ipc";
import { Button, Chip, Field, IconButton, Segmented, TextInput, cx, envClass } from "../../ui";
import { AgentRules } from "./AgentRules";
import { Note, Section, Switch } from "./controls";
import { type DraftErrors, type EnvDraft } from "./draft";
import { MissingSecretsList } from "./MissingSecrets";
import { SecretsList } from "./SecretsList";

export interface EnvEditorProps {
  draft: EnvDraft;
  onChange: (next: EnvDraft) => void;
  /** The stage this section belongs to (read-only flag, agent rules). */
  stage: StageDto;
  /** The saved section this draft edits; `undefined` for a new one. */
  live: ApplicationEnvDto | undefined;
  errors: DraftErrors;
  conflict: boolean;
  initialFill?: string;
  onReload: () => void;
  onKeepMine: () => void;
  onDelete: () => void;
}

export function EnvEditor({ draft, onChange, stage, live, errors, conflict, initialFill, onReload, onKeepMine, onDelete }: EnvEditorProps) {
  const [fill, setFill] = useState<{ name: string; n: number } | null>(initialFill ? { name: initialFill, n: 1 } : null);
  const readOnly = stage.readOnly;
  const set = (patch: Partial<EnvDraft>) => onChange({ ...draft, ...patch });
  const statuses: Record<string, SecretStatus> = Object.fromEntries((live?.secrets ?? []).map((s) => [s.name, s.status]));
  const missing = (live?.secrets ?? []).filter((s) => s.status === "missing");
  const production = draft.stage === "production";
  const varsChanged = !!live && JSON.stringify(live.variables.map((v) => [v.name, v.value])) !== JSON.stringify(draft.variables.map((v) => [v.name, v.value]));

  const setVar = (i: number, patch: Partial<EnvDraft["variables"][number]>) => set({ variables: draft.variables.map((v, j) => (j === i ? { ...v, ...patch } : v)) });

  return (
    <div className={cx("sh-editor", envClass(draft.stage))} data-kind={draft.stage}>
      {production && <div className="sh-prod-band" role="note"><i className="ph ph-warning" aria-hidden="true" /> Production. Changes here affect real systems.</div>}
      {readOnly && <Note tone="warn">This file was written by a newer Hopper. You can look, but not save it from this version.</Note>}
      {conflict && (
        <div className="sh-banner" role="alert">
          <p>This stage changed on disk after you opened it.</p>
          <div className="sh-row-actions">
            <Button size="sm" onClick={onReload}>Reload, drop my edits</Button>
            <Button size="sm" variant="primary" onClick={onKeepMine}>Keep mine</Button>
          </div>
        </div>
      )}

      {missing.length > 0 && (
        <MissingSecretsList items={missing.map((s) => ({ stage: draft.stage, application: draft.application, name: s.name }))} onPick={({ name }) => setFill({ name, n: (fill?.n ?? 0) + 1 })} />
      )}

      <Section title="Details">
        <Field label="Application" error={errors.application} hint={draft.isNew ? "Usually the name of a collection. Becomes part of the file name." : `Section of ${draft.stage}. Renaming is not supported.`}>
          {(id) => <TextInput id={id} list="env-apps" value={draft.application} disabled={!draft.isNew || readOnly} invalid={!!errors.application} placeholder="account" onChange={(e) => set({ application: e.target.value })} />}
        </Field>
      </Section>

      <Section title="Variables" aside={<span className="sh-aside">plain values, shared in git</span>}>
        <div className="sh-table-scroll">
          {draft.variables.length === 0 && <p className="sh-empty">No variables.</p>}
          {draft.variables.map((v, i) => (
            <div key={i} className="sh-var">
              <div className="sh-var-row">
                <TextInput mono aria-label="Variable name" placeholder="baseUrl" value={v.name} readOnly={v.local || readOnly} invalid={!!errors.variables[i]} onChange={(e) => setVar(i, { name: e.target.value })} />
                <TextInput mono aria-label={`Value of ${v.name || "variable"}`} placeholder="value" value={v.value} readOnly={v.local || readOnly} onChange={(e) => setVar(i, { value: e.target.value })} />
                {v.local ? <Chip tone="accent" title="This value comes from your *.local.yaml file">local</Chip> : <IconButton icon="trash" label={`Remove ${v.name || "variable"}`} size="sm" tone="danger" disabled={readOnly} onClick={() => set({ variables: draft.variables.filter((_, j) => j !== i) })} />}
              </div>
              {errors.variables[i] && <p className="ui-field-error" role="alert">{errors.variables[i]}</p>}
            </div>
          ))}
        </div>
        {!readOnly && <Button size="sm" icon="plus" onClick={() => set({ variables: [...draft.variables, { name: "", value: "", local: false }] })}>Add variable</Button>}
      </Section>

      <Section title="Secrets" aside={live && missing.length === 0 && live.secrets.length > 0 ? <Chip tone="ok">all set</Chip> : undefined}>
        <SecretsList env={{ stage: draft.stage, application: draft.application }} secrets={draft.secrets} statuses={statuses} errors={errors.secrets} readOnly={readOnly} fill={fill} onChange={(secrets) => set({ secrets })} />
      </Section>

      <Section title="Network safety">
        <Switch tone="danger" checked={draft.insecureSkipVerify} disabled={readOnly} onChange={(insecureSkipVerify) => set({ insecureSkipVerify })} label="Skip TLS certificate checks" hint="Accept any HTTPS certificate, including self-signed ones." />
        {draft.insecureSkipVerify && (
          <Note tone="danger">
            Risk: with checks off, Hopper cannot tell the real server from an impostor. Anyone on the network path can read and change the requests, including tokens and passwords you send.
            {production ? " This is production: do not do this." : " Use it only for servers you control, such as a local dev certificate."}
          </Note>
        )}
        <h4 className="sh-subtitle">Allowed hosts</h4>
        <p className="sh-help">Requests, and anything an agent builds, can only go to these hosts. This stops a secret from being sent somewhere you did not intend.</p>
        <Segmented
          ariaLabel="Allowed hosts source"
          value={draft.hostsExplicit ? "explicit" : "derived"}
          options={[{ value: "derived", label: "From base URL variables" }, { value: "explicit", label: "Explicit list", disabled: readOnly }]}
          onChange={(v) => set({ hostsExplicit: v === "explicit" })}
        />
        {draft.hostsExplicit ? (
          <HostList hosts={draft.hosts} errors={errors.hosts} readOnly={readOnly} onChange={(hosts) => set({ hosts })} />
        ) : (
          <>
            <div className="sh-hosts" aria-label="Derived hosts">
              {draft.hosts.length === 0 ? <span className="sh-empty">None yet. Add a base URL variable such as baseUrl.</span> : draft.hosts.map((h) => <code key={h} className="sh-host-chip">{h}</code>)}
            </div>
            {varsChanged && <p className="sh-help">Recalculated from your variables when you save.</p>}
          </>
        )}
      </Section>

      <Section title="Agent rules" aside={<span className="sh-aside">only you, whole stage</span>}>
        <AgentRules env={stage} />
      </Section>

      {live && !draft.isNew && (
        <Section title="Manage" tone="danger">
          <div className="sh-row-actions">
            <Button variant="danger" icon="trash" onClick={onDelete}>Delete application section</Button>
          </div>
        </Section>
      )}
    </div>
  );
}

function HostList({ hosts, errors, readOnly, onChange }: { hosts: string[]; errors: Record<number, string>; readOnly: boolean; onChange: (h: string[]) => void }): ReactNode {
  return (
    <div className="sh-hostlist">
      {hosts.map((h, i) => (
        <div key={i}>
          <div className="sh-host-row">
            <TextInput mono aria-label="Allowed host" placeholder="api.example.com" value={h} invalid={!!errors[i]} readOnly={readOnly} onChange={(e) => onChange(hosts.map((x, j) => (j === i ? e.target.value : x)))} />
            <IconButton icon="trash" label="Remove host" size="sm" tone="danger" disabled={readOnly} onClick={() => onChange(hosts.filter((_, j) => j !== i))} />
          </div>
          {errors[i] && <p className="ui-field-error" role="alert">{errors[i]}</p>}
        </div>
      ))}
      {hosts.length === 0 && <p className="sh-empty">No hosts: nothing can be sent from this environment.</p>}
      {!readOnly && <Button size="sm" icon="plus" onClick={() => onChange([...hosts, ""])}>Add host</Button>}
    </div>
  );
}
