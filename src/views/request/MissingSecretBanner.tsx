import { useState, type FormEvent } from "react";
import type { Auth, RequestDoc } from "../../ipc";
import { openSheet, useStore } from "../../store";
import { attempt } from "../../sheets/collection/controls";
import { setSecret } from "../../sheets/collection/ops";
import { Button, Icon, TextInput } from "../../ui";
import { missingSecretsUsed, type VarInfo } from "./logic";

/** The request uses a secret that has no value in the active environment: ask for it right here instead of failing at Send. */
export function MissingSecretBanner({ doc, info, collection, inherited, onSend, busy }: { doc: RequestDoc; info: VarInfo; collection: string | null; inherited?: Auth; onSend: () => void; busy: boolean }) {
  const stage = useStore((s) => s.environments.find((e) => e.active) ?? null);
  const names = missingSecretsUsed(doc, info, inherited);
  const [value, setValue] = useState("");
  const [saving, setSaving] = useState(false);
  const name = names[0];
  if (!name || !stage) return null;

  const save = async (andSend: boolean) => {
    if (!value || !collection) return;
    setSaving(true);
    const ok = await attempt(() => setSecret(stage.stage, collection, name, value), `${name} saved for ${stage.stage}`);
    setSaving(false);
    if (!ok) return;
    setValue("");
    if (andSend) onSend();
  };

  return (
    <form className="mb" role="group" aria-label={`${name} has no value`} onSubmit={(e: FormEvent) => (e.preventDefault(), void save(false))}>
      <Icon name="warning" />
      <div className="mb-body">
        <b><code>{`{{${name}}}`}</code> has no value in {stage.stage}</b>
        <p>It would be sent empty. Add it here; it is stored on this machine only.{names.length > 1 ? ` ${names.length - 1} more after this one.` : ""}</p>
        {collection && !stage.readOnly ? (
          <div className="cv-inline">
            <TextInput type="password" mono autoComplete="new-password" aria-label={`Value of ${name} in ${stage.stage}`} placeholder="Paste the value" value={value} onChange={(e) => setValue(e.target.value)} />
            <Button type="submit" size="sm" loading={saving} disabled={!value}>Save</Button>
            <Button size="sm" variant="primary" disabled={!value || saving || busy} onClick={() => void save(true)}>Save &amp; send</Button>
          </div>
        ) : (
          <Button size="sm" icon="lock-simple" onClick={() => openSheet("environments")}>Open Environments</Button>
        )}
      </div>
    </form>
  );
}
