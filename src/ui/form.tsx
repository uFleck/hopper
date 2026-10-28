import { useId, type InputHTMLAttributes, type KeyboardEvent, type ReactNode, type Ref } from "react";
import { cx } from "./primitives";

export interface FieldProps {
  label: string;
  hint?: string;
  /** Shown in red under the control; also marks the control invalid when you pass `invalid` to it. */
  error?: string | null;
  /** Receives the id to put on the control so the label points to it. */
  children: (id: string) => ReactNode;
  className?: string;
}

/** A labelled form row: `<Field label="Name">{(id) => <TextInput id={id} />}</Field>`. */
export function Field({ label, hint, error, children, className }: FieldProps) {
  const id = useId();
  return (
    <div className={cx("ui-field", className)}>
      <label className="ui-field-label" htmlFor={id}>{label}</label>
      {children(id)}
      {error ? <p className="ui-field-error" role="alert">{error}</p> : hint ? <p className="ui-field-hint">{hint}</p> : null}
    </div>
  );
}

export interface TextInputProps extends InputHTMLAttributes<HTMLInputElement> {
  invalid?: boolean;
  mono?: boolean;
  ref?: Ref<HTMLInputElement>;
}

export function TextInput({ invalid, mono, className, ref, ...rest }: TextInputProps) {
  return <input ref={ref} className={cx("ui-input", mono && "ui-input--mono", invalid && "is-invalid", className)} aria-invalid={invalid || undefined} spellCheck={false} {...rest} />;
}

// ------------------------------------------------------------------------------------ segmented, tabs

export interface Option<T extends string> {
  value: T;
  label: ReactNode;
  disabled?: boolean;
}

function arrowNav<T extends string>(e: KeyboardEvent, options: Option<T>[], value: T, onChange: (v: T) => void) {
  const dir = e.key === "ArrowRight" || e.key === "ArrowDown" ? 1 : e.key === "ArrowLeft" || e.key === "ArrowUp" ? -1 : 0;
  if (!dir) return;
  e.preventDefault();
  const enabled = options.filter((o) => !o.disabled);
  const at = enabled.findIndex((o) => o.value === value);
  const next = enabled[(at + dir + enabled.length) % enabled.length];
  if (next) {
    onChange(next.value);
    const btns = (e.currentTarget as HTMLElement).querySelectorAll<HTMLElement>("[role=radio],[role=tab]");
    btns[options.indexOf(next)]?.focus();
  }
}

/** A short either-or switch, e.g. Pretty / Raw. */
export function Segmented<T extends string>({ value, options, onChange, ariaLabel, size = "md" }: { value: T; options: Option<T>[]; onChange: (v: T) => void; ariaLabel: string; size?: "sm" | "md" }) {
  return (
    <div className={cx("ui-seg", size === "sm" && "ui-seg--sm")} role="radiogroup" aria-label={ariaLabel} onKeyDown={(e) => arrowNav(e, options, value, onChange)}>
      {options.map((o) => (
        <button key={o.value} type="button" role="radio" aria-checked={o.value === value} tabIndex={o.value === value ? 0 : -1} disabled={o.disabled} className={cx("ui-seg-item", o.value === value && "is-on")} onClick={() => onChange(o.value)}>
          {o.label}
        </button>
      ))}
    </div>
  );
}

export interface TabItem<T extends string> extends Option<T> {
  /** A count or dot after the label, e.g. the number of headers. */
  badge?: ReactNode;
}

/** A tab bar. It only switches `value`; render the matching content yourself (give it `role="tabpanel"`). */
export function Tabs<T extends string>({ value, tabs, onChange, ariaLabel }: { value: T; tabs: TabItem<T>[]; onChange: (v: T) => void; ariaLabel: string }) {
  return (
    <div className="ui-tabs" role="tablist" aria-label={ariaLabel} onKeyDown={(e) => arrowNav(e, tabs, value, onChange)}>
      {tabs.map((t) => (
        <button key={t.value} type="button" role="tab" aria-selected={t.value === value} tabIndex={t.value === value ? 0 : -1} disabled={t.disabled} className={cx("ui-tab", t.value === value && "is-on")} onClick={() => onChange(t.value)}>
          {t.label}
          {t.badge != null && <span className="ui-tab-badge">{t.badge}</span>}
        </button>
      ))}
    </div>
  );
}
