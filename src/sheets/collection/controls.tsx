import { useState, type FormEvent } from "react";
import { toHopperError } from "../../ipc";
import { toast } from "../../store";
import { Button, TextInput, cx } from "../../ui";

/** Runs a write, shows a failure as a toast, and says whether it went through. */
export async function attempt(work: () => Promise<unknown>, done?: string): Promise<boolean> {
  try {
    await work();
    if (done) toast(done, "success");
    return true;
  } catch (e) {
    toast(toHopperError(e).message, "error");
    return false;
  }
}

export function Switch({ checked, onChange, label, hint }: { checked: boolean; onChange: (on: boolean) => void; label: string; hint?: string }) {
  return (
    <div className="cv-switchrow">
      <button type="button" role="switch" aria-checked={checked} aria-label={label} className={cx("cv-switch", checked && "is-on")} onClick={() => onChange(!checked)} />
      <span>{label}</span>
      {hint && <span className="cv-muted">{hint}</span>}
    </div>
  );
}

export function Segmented<T extends string>({ value, options, onChange, label }: { value: T; options: { value: T; label: string }[]; onChange: (v: T) => void; label: string }) {
  return (
    <div className="cv-seg" role="radiogroup" aria-label={label}>
      {options.map((o) => (
        <button key={o.value} type="button" role="radio" aria-checked={o.value === value} className={cx(o.value === value && "is-on")} onClick={() => onChange(o.value)}>
          {o.label}
        </button>
      ))}
    </div>
  );
}

/** One masked field and a Save button: the value goes to the keyring and is never shown again. */
export function SecretValueForm({ label, submitLabel = "Save", onSave, onCancel, autoFocus = true }: { label: string; submitLabel?: string; onSave: (value: string) => Promise<boolean>; onCancel?: () => void; autoFocus?: boolean }) {
  const [value, setValue] = useState("");
  const [busy, setBusy] = useState(false);
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (!value) return;
    setBusy(true);
    const ok = await onSave(value);
    setBusy(false);
    if (ok) setValue("");
  };
  return (
    <form className="cv-inline" onSubmit={(e) => void submit(e)} onKeyDown={(e) => e.key === "Escape" && onCancel && (e.preventDefault(), e.stopPropagation(), onCancel())}>
      <TextInput type="password" mono autoComplete="new-password" aria-label={label} placeholder="Paste the value" autoFocus={autoFocus} value={value} onChange={(e) => setValue(e.target.value)} />
      <Button type="submit" size="sm" variant="primary" loading={busy} disabled={!value}>{submitLabel}</Button>
      {onCancel && <Button size="sm" variant="ghost" onClick={onCancel}>Cancel</Button>}
    </form>
  );
}
