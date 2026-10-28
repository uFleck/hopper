import { useCallback, useState, type ReactNode } from "react";
import { Icon, cx } from "../../ui";

const KEY = "hopper.request.sections";

const read = (): Record<string, boolean> => {
  try {
    return JSON.parse(localStorage.getItem(KEY) ?? "{}") as Record<string, boolean>;
  } catch {
    return {};
  }
};

/** Which sections are open, remembered across requests and restarts. */
export function useSectionOpen(id: string, fallback: boolean): [boolean, (open: boolean) => void] {
  const [open, setOpen] = useState(() => read()[id] ?? fallback);
  const set = useCallback(
    (next: boolean) => {
      setOpen(next);
      try {
        localStorage.setItem(KEY, JSON.stringify({ ...read(), [id]: next }));
      } catch {
        /* storage unavailable: the choice lasts for this session */
      }
    },
    [id],
  );
  return [open, set];
}

export interface SectionProps {
  id: string;
  title: string;
  defaultOpen?: boolean;
  /** A count or short status after the title. */
  badge?: ReactNode;
  tone?: "warn";
  actions?: ReactNode;
  children: ReactNode;
}

export function Section({ id, title, defaultOpen = false, badge, tone, actions, children }: SectionProps) {
  const [open, setOpen] = useSectionOpen(id, defaultOpen);
  const bodyId = `rv-sec-${id}`;
  return (
    <section className={cx("rv-sec", open && "is-open", tone && `rv-sec--${tone}`)}>
      <header className="rv-sec-head">
        <button type="button" className="rv-sec-toggle" aria-expanded={open} aria-controls={bodyId} onClick={() => setOpen(!open)}>
          <Icon name="caret-right" className="rv-sec-caret" />
          <span className="rv-sec-title">{title}</span>
          {badge != null && badge !== 0 && badge !== false && <span className="rv-sec-badge">{badge}</span>}
        </button>
        {open && actions}
      </header>
      {open && (
        <div className="rv-sec-body" id={bodyId}>
          {children}
        </div>
      )}
    </section>
  );
}
