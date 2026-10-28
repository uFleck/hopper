import { useEffect, useRef, useState, type FormEvent } from "react";
import { toHopperError, type SecretStatus } from "../../ipc";
import { refreshEnvironments, toast } from "../../store";
import { Button, IconButton, TextInput, cx, confirm } from "../../ui";
import { REVEAL_SECONDS, type AppScope, clearSecretValue, revealSecretValue, setSecretValue } from "./api";
import { Note } from "./controls";

export interface SecretsListProps {
  env: AppScope;
  secrets: { name: string; provider: string }[];
  /** Status of secrets that exist on disk; a name missing here is not saved yet. */
  statuses: Record<string, SecretStatus>;
  errors: Record<number, string>;
  readOnly: boolean;
  /** Asks one row to open its "set value" field (from the checklist). */
  fill: { name: string; n: number } | null;
  onChange: (next: { name: string; provider: string }[]) => void;
}

export function SecretsList({ env, secrets, statuses, errors, readOnly, fill, onChange }: SecretsListProps) {
  const rename = (i: number, name: string) => onChange(secrets.map((s, j) => (j === i ? { ...s, name } : s)));
  return (
    <div className="sh-secrets">
      {secrets.length === 0 && <p className="sh-empty">No secrets. Add one for tokens and passwords; values live in the OS credential store.</p>}
      {secrets.map((s, i) => {
        const status = statuses[s.name];
        return (
          <SecretRow
            key={i}
            env={env}
            name={s.name}
            provider={s.provider}
            status={status}
            error={errors[i]}
            readOnly={readOnly}
            fill={fill?.name === s.name ? fill.n : 0}
            onRename={(name) => rename(i, name)}
            onRemove={() => onChange(secrets.filter((_, j) => j !== i))}
          />
        );
      })}
      {!readOnly && (
        <Button size="sm" icon="plus" onClick={() => onChange([...secrets, { name: "", provider: "keyring" }])}>
          Add secret
        </Button>
      )}
    </div>
  );
}

interface RowProps {
  env: AppScope;
  name: string;
  provider: string;
  status: SecretStatus | undefined;
  error?: string;
  readOnly: boolean;
  fill: number;
  onRename: (name: string) => void;
  onRemove: () => void;
}

function SecretRow({ env, name, provider, status, error, readOnly, fill, onRename, onRemove }: RowProps) {
  const saved = status !== undefined;
  const [editing, setEditing] = useState(false);
  const [value, setValue] = useState("");
  const [busy, setBusy] = useState(false);
  const [revealed, setRevealed] = useState<{ value: string; until: number } | null>(null);
  const [left, setLeft] = useState(0);
  const field = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (fill && saved) setEditing(true);
  }, [fill, saved]);

  useEffect(() => {
    if (editing) field.current?.focus();
  }, [editing]);

  useEffect(() => {
    if (!revealed) return;
    const tick = () => {
      const ms = revealed.until - Date.now();
      if (ms <= 0) setRevealed(null);
      else setLeft(Math.ceil(ms / 1000));
    };
    tick();
    const t = setInterval(tick, 250);
    return () => clearInterval(t);
  }, [revealed]);

  useEffect(() => {
    if (status !== "set") setRevealed(null);
  }, [status]);

  const close = () => {
    setEditing(false);
    setValue("");
  };

  const save = async (e: FormEvent) => {
    e.preventDefault();
    if (!value) return;
    setBusy(true);
    try {
      await setSecretValue(env, name, value);
      close();
      await refreshEnvironments();
      toast(`${name} saved`, "success");
    } catch (err) {
      toast(toHopperError(err).message, "error");
    } finally {
      setBusy(false);
    }
  };

  const clear = async () => {
    const ok = await confirm({ title: `Clear ${name}?`, message: `The stored value for ${name} in ${env.stage}/${env.application} is deleted from this machine. Requests that need it will fail until you set it again.`, confirmLabel: "Clear value", tone: "danger" });
    if (!ok) return;
    try {
      await clearSecretValue(env, name);
      await refreshEnvironments();
    } catch (err) {
      toast(toHopperError(err).message, "error");
    }
  };

  const reveal = async () => {
    try {
      const v = await revealSecretValue(env, name);
      setRevealed({ value: v, until: Date.now() + REVEAL_SECONDS * 1000 });
    } catch (err) {
      toast(toHopperError(err).message, "error");
    }
  };

  return (
    <div className={cx("sh-secret", status === "missing" && "is-missing")}>
      <div className="sh-secret-main">
        <span className={cx("sh-dot", status === "set" ? "is-set" : "is-missing")} role="img" aria-label={status === "set" ? "Value set" : status === "missing" ? "Value missing" : "Not saved yet"} title={status === "set" ? "Set" : status === "missing" ? "Missing" : "Save the environment first"} />
        {saved ? <span className="sh-secret-name">{name}</span> : <TextInput mono aria-label="Secret name" placeholder="apiKey" value={name} invalid={!!error} disabled={readOnly} onChange={(e) => onRename(e.target.value)} />}
        <span className="sh-secret-value" aria-label={status === "set" ? "Value hidden" : undefined}>{status === "set" ? "••••••••" : status === "missing" ? "missing" : "not saved"}</span>
        <span className="sh-secret-prov">{provider}</span>
        <span className="sh-secret-actions">
          {status === "set" && <IconButton icon="eye" label={`Reveal ${name} for ${REVEAL_SECONDS} seconds`} size="sm" onClick={reveal} disabled={revealed !== null} />}
          {saved && !readOnly && <Button size="sm" onClick={() => setEditing((v) => !v)} aria-expanded={editing}>{status === "set" ? "Replace" : "Set value"}</Button>}
          {status === "set" && !readOnly && <IconButton icon="eraser" label={`Clear ${name}`} size="sm" tone="danger" onClick={clear} />}
          {!readOnly && !saved && <IconButton icon="trash" label="Remove secret" size="sm" tone="danger" onClick={onRemove} />}
          {!readOnly && saved && <IconButton icon="x" label={`Remove ${name} from the environment`} size="sm" tone="danger" onClick={onRemove} />}
        </span>
      </div>
      {error && <p className="ui-field-error" role="alert">{error}</p>}
      {!saved && name && !error && <p className="sh-secret-hint">Save the environment, then set its value.</p>}
      {revealed && (
        <div className="sh-reveal" role="status">
          <code className="sh-reveal-value">{revealed.value}</code>
          <span className="sh-reveal-count">Hides in {left}s</span>
          <Button size="sm" variant="ghost" onClick={() => setRevealed(null)}>Hide now</Button>
        </div>
      )}
      {editing && (
        <form className="sh-secret-form" onSubmit={save} onKeyDown={(e) => {
          if (e.key === "Escape") {
            e.preventDefault();
            e.stopPropagation();
            close();
          }
        }}>
          <TextInput ref={field} type="password" mono autoComplete="new-password" aria-label={`New value for ${name}`} placeholder="Paste the value" value={value} onChange={(e) => setValue(e.target.value)} />
          <Button type="submit" size="sm" variant="primary" loading={busy} disabled={!value}>Save</Button>
          <Button size="sm" variant="ghost" onClick={close}>Cancel</Button>
          <Note>Stored in the OS credential store on this machine. It is never shown again unless you press Reveal.</Note>
        </form>
      )}
    </div>
  );
}
