import type { ButtonHTMLAttributes, ReactNode, Ref } from "react";
import type { Method, Stage } from "../ipc/types";

export const cx = (...parts: (string | false | null | undefined)[]) => parts.filter(Boolean).join(" ");

/** A Phosphor icon by name without the prefix: `<Icon name="plus" />`. */
export function Icon({ name, className }: { name: string; className?: string }) {
  return <i className={cx("ph", `ph-${name}`, className)} aria-hidden="true" />;
}

export function Spinner({ size = 14, label }: { size?: number; label?: string }) {
  return <span className="ui-spinner" style={{ width: size, height: size }} role={label ? "status" : undefined} aria-label={label} aria-hidden={label ? undefined : true} />;
}

export function Kbd({ children }: { children: ReactNode }) {
  return <kbd className="ui-kbd">{children}</kbd>;
}

// ------------------------------------------------------------------------------------------ buttons

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: "primary" | "secondary" | "ghost" | "danger";
  size?: "sm" | "md";
  /** Phosphor icon name shown before the label. */
  icon?: string;
  /** Shows a spinner and ignores clicks. */
  loading?: boolean;
  ref?: Ref<HTMLButtonElement>;
}

export function Button({ variant = "secondary", size = "md", icon, loading, className, children, disabled, type = "button", ref, ...rest }: ButtonProps) {
  return (
    <button ref={ref} type={type} className={cx("ui-btn", `ui-btn--${variant}`, size === "sm" && "ui-btn--sm", className)} disabled={disabled || loading} aria-busy={loading || undefined} {...rest}>
      {loading ? <Spinner /> : icon ? <Icon name={icon} /> : null}
      {children != null && <span className="ui-btn-label">{children}</span>}
    </button>
  );
}

export interface IconButtonProps extends Omit<ButtonHTMLAttributes<HTMLButtonElement>, "children"> {
  icon: string;
  /** Required: it is the accessible name and the tooltip. */
  label: string;
  size?: "sm" | "md";
  active?: boolean;
  tone?: "default" | "danger";
}

export function IconButton({ icon, label, size = "md", active, tone = "default", className, type = "button", ...rest }: IconButtonProps) {
  return (
    <button type={type} className={cx("ui-iconbtn", size === "sm" && "ui-iconbtn--sm", active && "is-active", tone === "danger" && "ui-iconbtn--danger", className)} aria-label={label} data-tip={label} aria-pressed={active} {...rest}>
      <Icon name={icon} />
    </button>
  );
}

// -------------------------------------------------------------------------------------------- chips

const METHOD_CLASS: Record<Method, string> = {
  GET: "get",
  POST: "post",
  PUT: "put",
  PATCH: "patch",
  DELETE: "delete",
  HEAD: "muted",
  OPTIONS: "muted",
};

export const methodClass = (m: Method) => `tone-${METHOD_CLASS[m]}`;
export const envClass = (stage: Stage) => `env-${stage}`;

export type Tone = "neutral" | "ok" | "warn" | "err" | "accent";

export type ChipProps =
  | { method: Method; env?: never; tone?: never; children?: ReactNode; className?: string; tip?: string }
  | { env: Stage; method?: never; tone?: never; children?: ReactNode; className?: string; tip?: string }
  | { tone?: Tone; method?: never; env?: never; children?: ReactNode; className?: string; tip?: string };

/** A small label. `method` shows the HTTP verb in its colour, `env` takes the environment kind's colour (production is red), else a tone. */
export function Chip(props: ChipProps) {
  const { children, className, tip } = props;
  if (props.method) return <span className={cx("ui-chip", "ui-chip--method", methodClass(props.method), className)} data-tip={tip}>{children ?? props.method}</span>;
  if (props.env) return <span className={cx("ui-chip", "ui-chip--env", envClass(props.env), className)} data-tip={tip}>{children ?? props.env}</span>;
  return <span className={cx("ui-chip", `tone-${props.tone ?? "neutral"}`, className)} data-tip={tip}>{children}</span>;
}

/** The colour of an HTTP status: 2xx ok, 3xx accent, 4xx warn, 5xx and network errors red. */
export function statusTone(status: number): Tone {
  if (status >= 500 || status === 0) return "err";
  if (status >= 400) return "warn";
  if (status >= 300) return "accent";
  return "ok";
}
