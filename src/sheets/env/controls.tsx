import { useId, type ReactNode } from "react";
import { cx } from "../../ui";

export interface SwitchProps {
  checked: boolean;
  onChange: (next: boolean) => void;
  label: ReactNode;
  hint?: ReactNode;
  disabled?: boolean;
  tone?: "default" | "danger";
}

/** A labelled on/off switch row. Space or Enter toggles it when focused. */
export function Switch({ checked, onChange, label, hint, disabled, tone = "default" }: SwitchProps) {
  const id = useId();
  return (
    <div className={cx("sh-switch", disabled && "is-disabled")}>
      <button type="button" role="switch" id={id} aria-checked={checked} aria-describedby={hint ? `${id}-hint` : undefined} disabled={disabled} className={cx("sh-switch-btn", checked && "is-on", tone === "danger" && "is-danger")} onClick={() => onChange(!checked)}>
        <span className="sh-switch-knob" />
      </button>
      <label htmlFor={id} className="sh-switch-text">
        <span className="sh-switch-label">{label}</span>
        {hint && <span id={`${id}-hint`} className="sh-switch-hint">{hint}</span>}
      </label>
    </div>
  );
}

/** A titled block inside a sheet. */
export function Section({ title, aside, children, tone }: { title: string; aside?: ReactNode; children: ReactNode; tone?: "danger" }) {
  const id = useId();
  return (
    <section className={cx("sh-section", tone === "danger" && "sh-section--danger")} aria-labelledby={id}>
      <header className="sh-section-head">
        <h3 id={id} className="sh-section-title">{title}</h3>
        {aside}
      </header>
      {children}
    </section>
  );
}

export function Note({ tone = "info", children }: { tone?: "info" | "warn" | "danger"; children: ReactNode }) {
  return <p className={cx("sh-note", `sh-note--${tone}`)} role={tone === "info" ? undefined : "note"}>{children}</p>;
}

export async function copyText(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    return false;
  }
}
