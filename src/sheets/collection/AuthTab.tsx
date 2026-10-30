import { useEffect, useMemo, useState } from "react";
import type { Auth, Stage } from "../../ipc";
import { loadCollectionAuth, loadVariables, saveCollectionAuth, targetKey, toast, useStore } from "../../store";
import { Button, Icon, TextInput } from "../../ui";
import { AuthSection } from "../../views/request/AuthSection";
import { authText, switchAuth, variablesIn, type AuthType } from "../../views/request/logic";
import { useCollectionInfo } from "../../views/request/useCollectionInfo";
import { Segmented, SecretValueForm, attempt } from "./controls";
import { STAGES, STAGE_LABEL, buildMatrix, isRawValue, nameProblem, type MatrixRow } from "./matrix";
import { addSecret, setSecret } from "./ops";

const TYPES: { value: AuthType; label: string }[] = [
  { value: "none", label: "None" },
  { value: "bearer", label: "Bearer" },
  { value: "basic", label: "Basic" },
  { value: "apiKey", label: "API key" },
];

interface Field {
  label: string;
  value: string;
  suggest: string;
  set: (value: string) => Auth;
}

/** The auth fields that hold a credential, and how to rewrite each. */
function credentialFields(a: Auth): Field[] {
  switch (a.type) {
    case "bearer": return [{ label: "token", value: a.token, suggest: "apiToken", set: (token) => ({ ...a, token }) }];
    case "basic": return [{ label: "password", value: a.password, suggest: "password", set: (password) => ({ ...a, password }) }];
    case "apiKey": return [{ label: "API key", value: a.value, suggest: "apiKey", set: (value) => ({ ...a, value }) }];
    default: return [];
  }
}

function sentPreview(a: Auth): string | null {
  switch (a.type) {
    case "bearer": return "Authorization: Bearer ••••••••";
    case "basic": return "Authorization: Basic ••••••••";
    case "apiKey": return a.in === "query" ? `?${a.name || "key"}=••••••••` : `${a.name || "X-Api-Key"}: ••••••••`;
    default: return null;
  }
}

/** The authentication every request of the collection inherits. Pasting a real credential offers to store it as a secret instead. */
export function AuthTab({ slug, name, requests }: { slug: string; name: string; requests: number }) {
  const loaded = useStore((s) => s.collectionAuth[slug]);
  const defaults = useStore((s) => s.scopeVars[targetKey({ type: "collection", slug })]);
  const stages = useStore((s) => s.environments);
  const active = stages.find((s) => s.active)?.stage ?? null;
  const [edited, setEdited] = useState<Auth | null>(null);
  const [busy, setBusy] = useState(false);
  const [fill, setFill] = useState<{ name: string; stage: Stage } | null>(null);
  const info = useCollectionInfo(slug, true);

  useEffect(() => {
    void loadCollectionAuth(slug);
    void loadVariables({ type: "collection", slug });
  }, [slug]);

  const saved = loaded?.auth;
  const draft: Auth = edited ?? (saved?.type === "inherit" ? { type: "none" } : saved) ?? { type: "none" };
  const dirty = edited !== null && JSON.stringify(edited) !== JSON.stringify(saved ?? { type: "none" });
  const rows = useMemo(() => buildMatrix(defaults?.variables ?? [], stages, slug), [defaults, stages, slug]);
  const raw = credentialFields(draft).find((f) => isRawValue(f.value));
  const used = rows.filter((r) => r.secret && authText(draft).some((t) => variablesIn(t).includes(r.name)));
  const preview = sentPreview(draft);

  const save = async () => {
    setBusy(true);
    const ok = await saveCollectionAuth(slug, draft, loaded?.revision ?? null);
    setBusy(false);
    if (ok) {
      setEdited(null);
      toast(`Saved authentication of ${name}`, "success");
    }
  };

  return (
    <div className="cv">
      <Segmented<AuthType> label="Auth type" value={draft.type} options={TYPES} onChange={(t) => setEdited(switchAuth(draft, t))} />

      {draft.type === "none" ? (
        <p className="cv-muted">No authentication is added. Requests that need it set their own.</p>
      ) : (
        <>
          <AuthSection scope="collection" hideType auth={draft} onChange={setEdited} info={info} readOnly={false} />
          {raw && <SaveAsSecret key={raw.label} field={raw} slug={slug} rows={rows} active={active} onReplaced={(token) => setEdited(raw.set(token))} />}
        </>
      )}

      {used.length > 0 && (
        <div className="cv-card">
          <span className="cv-lbl">Value in each environment</span>
          {used.map((r) => (
            <div key={r.name} className="cv-statuses">
              <code>{r.name}</code>
              {STAGES.map((s) => {
                const c = r.cells[s];
                const ok = c.kind === "set";
                return (
                  <div key={s} className="cv-envrow cv-statusrow">
                    <span className={s === active ? "is-active" : undefined}>{STAGE_LABEL[s]}</span>
                    <span className={ok ? "cv-ok" : "cv-gap"}>{ok ? "● has a value" : "○ not set"}</span>
                    <Button size="sm" variant={ok ? "ghost" : undefined} onClick={() => setFill({ name: r.name, stage: s })}>{ok ? "Replace" : "Set value"}</Button>
                  </div>
                );
              })}
              {fill?.name === r.name && (
                <SecretValueForm label={`Value of ${r.name} in ${STAGE_LABEL[fill.stage]}`} submitLabel={`Save for ${STAGE_LABEL[fill.stage]}`} onCancel={() => setFill(null)} onSave={async (v) => {
                  const ok = await attempt(() => setSecret(fill.stage, slug, r.name, v), `${r.name} saved for ${STAGE_LABEL[fill.stage]}`);
                  if (ok) setFill(null);
                  return ok;
                }} />
              )}
            </div>
          ))}
        </div>
      )}

      {preview && (
        <div className="cv-field">
          <span className="cv-lbl">What gets sent</span>
          <code className="cv-preview">{preview}</code>
        </div>
      )}

      <p className="cv-muted">
        Applies to {requests} request{requests === 1 ? "" : "s"} set to Inherit. Changing it pauses Auto agent grants for this collection until you approve again.
      </p>

      {dirty && (
        <div className="cv-actions">
          <Button variant="ghost" disabled={busy} onClick={() => setEdited(null)}>Discard</Button>
          <Button variant="primary" loading={busy} disabled={!!raw} data-tip={raw ? "Store the credential as a secret first" : undefined} onClick={() => void save()}>Save</Button>
        </div>
      )}
    </div>
  );
}

function SaveAsSecret({ field, slug, rows, active, onReplaced }: { field: Field; slug: string; rows: MatrixRow[]; active: Stage | null; onReplaced: (token: string) => void }) {
  const [name, setName] = useState(() => (rows.some((r) => r.name === field.suggest) ? `${field.suggest}2` : field.suggest));
  const [busy, setBusy] = useState(false);
  const problem = nameProblem(name.trim(), rows);

  const store = async () => {
    setBusy(true);
    const n = name.trim();
    const ok = await attempt(() => addSecret(slug, n, active ? { [active]: field.value } : {}), `Stored ${n} as a secret${active ? ` in ${STAGE_LABEL[active]}` : ""}`);
    setBusy(false);
    if (ok) onReplaced(`{{${n}}}`);
  };

  return (
    <div className="cv-warn" role="group" aria-label="Store the credential as a secret">
      <Icon name="warning" />
      <div>
        <b>This looks like a real {field.label}.</b>
        <p>The collection file is shared in git. Store it as a secret and the request will use a variable instead.{active ? ` The value is saved for ${STAGE_LABEL[active]}.` : ""}</p>
        <div className="cv-inline">
          <TextInput mono aria-label="Name of the secret" value={name} invalid={!!problem} onChange={(e) => setName(e.target.value)} />
          <Button variant="primary" size="sm" loading={busy} disabled={!name.trim() || !!problem} onClick={() => void store()}>Save as secret</Button>
        </div>
        {problem && <span className="cv-bad" role="alert">{problem}</span>}
      </div>
    </div>
  );
}
