import { useEffect, useId, useRef, useState, type ReactNode } from "react";
import { Icon, cx } from "../../ui";

export interface MenuItem {
  label: string;
  hint?: string;
  icon?: string;
  title?: string;
  tone?: "default" | "danger" | "warn";
  checked?: boolean;
  disabled?: boolean;
  onSelect: () => void;
}

export interface MenuProps {
  /** The trigger button content. */
  trigger: ReactNode;
  ariaLabel: string;
  items: MenuItem[];
  triggerClassName?: string;
  tip?: string;
  disabled?: boolean;
  align?: "start" | "end";
}

/** A small popover menu: Enter/Space/Down opens, arrows move, Esc closes without dismissing the panel. */
export function Menu({ trigger, ariaLabel, items, triggerClassName, tip, disabled, align = "end" }: MenuProps) {
  const [open, setOpen] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  const id = useId();

  useEffect(() => {
    if (!open) return;
    const away = (e: MouseEvent) => !root.current?.contains(e.target as Node) && setOpen(false);
    document.addEventListener("mousedown", away);
    root.current?.querySelector<HTMLElement>('[role="menuitem"]:not([disabled])')?.focus();
    return () => document.removeEventListener("mousedown", away);
  }, [open]);

  const move = (dir: 1 | -1) => {
    const els = [...(root.current?.querySelectorAll<HTMLElement>('[role="menuitem"]:not([disabled])') ?? [])];
    const at = els.indexOf(document.activeElement as HTMLElement);
    els[(at + dir + els.length) % els.length]?.focus();
  };

  const onKey = (e: React.KeyboardEvent) => {
    if (e.key === "Escape" && open) {
      e.preventDefault();
      e.stopPropagation();
      setOpen(false);
      root.current?.querySelector<HTMLElement>("button")?.focus();
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      if (open) move(1);
      else setOpen(true);
    } else if (e.key === "ArrowUp" && open) {
      e.preventDefault();
      move(-1);
    } else if (e.key === "Tab" && open) setOpen(false);
  };

  return (
    <div className="rv-menu" ref={root} onKeyDown={onKey}>
      <button type="button" className={triggerClassName} aria-label={ariaLabel} data-tip={tip ?? ariaLabel} aria-haspopup="menu" aria-expanded={open} aria-controls={open ? id : undefined} disabled={disabled} onClick={() => setOpen((o) => !o)}>
        {trigger}
      </button>
      {open && (
        <div className={cx("rv-menu-list", align === "start" && "rv-menu-list--start")} id={id} role="menu" aria-label={ariaLabel}>
          {items.map((it) => (
            <button
              key={it.label}
              type="button"
              role="menuitem"
              className={cx("rv-menu-item", it.tone && `rv-menu-item--${it.tone}`)}
              disabled={it.disabled}
              data-tip={it.title}
              onClick={() => {
                setOpen(false);
                it.onSelect();
                root.current?.querySelector<HTMLElement>("button")?.focus();
              }}
            >
              <span className="rv-menu-icon">{it.checked ? <Icon name="check" /> : it.icon ? <Icon name={it.icon} /> : null}</span>
              <span className="rv-menu-text">
                <span>{it.label}</span>
                {it.hint && <span className="rv-menu-hint">{it.hint}</span>}
              </span>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
