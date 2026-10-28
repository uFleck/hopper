import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { cx } from "../../ui";

export interface MenuItem {
  key: string;
  label: ReactNode;
  icon?: string;
  /** Right-aligned extra, e.g. a badge or chip. */
  trailing?: ReactNode;
  onSelect: () => void;
  tone?: "default" | "danger";
  selected?: boolean;
  disabled?: boolean;
  /** Draws a divider above the item. */
  separated?: boolean;
}

export interface MenuProps {
  /** The button that opens it; spread `props` on the button. */
  trigger: (props: { onClick: () => void; "aria-haspopup": "menu"; "aria-expanded": boolean; ref: (el: HTMLButtonElement | null) => void }, open: boolean) => ReactNode;
  items: MenuItem[];
  label: string;
  align?: "start" | "end";
  className?: string;
}

const WIDTH = 240;

/** A popover list of actions. Fixed-positioned from its trigger so scroll containers do not clip it. Esc closes it without dismissing the panel. */
export function Menu({ trigger, items, label, align = "end", className }: MenuProps) {
  const [open, setOpen] = useState(false);
  const [pos, setPos] = useState<{ top?: number; bottom?: number; left: number; maxHeight: number }>({ left: 0, maxHeight: 320 });
  const button = useRef<HTMLButtonElement | null>(null);
  const list = useRef<HTMLDivElement>(null);

  useLayoutEffect(() => {
    if (!open || !button.current) return;
    const r = button.current.getBoundingClientRect();
    const width = Math.min(WIDTH, window.innerWidth - 16);
    const left = Math.max(8, Math.min(align === "end" ? r.right - width : r.left, window.innerWidth - width - 8));
    const below = window.innerHeight - r.bottom - 8;
    const above = r.top - 8;
    if (below >= 160 || below >= above) setPos({ top: r.bottom + 4, left, maxHeight: Math.max(120, below - 4) });
    else setPos({ bottom: window.innerHeight - r.top + 4, left, maxHeight: Math.max(120, above - 4) });
  }, [open, align]);

  useEffect(() => {
    if (!open) return;
    list.current?.querySelector<HTMLElement>("[role=menuitem]:not([disabled])")?.focus();
    const away = (e: MouseEvent) => {
      if (!list.current?.contains(e.target as Node) && !button.current?.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener("mousedown", away);
    return () => document.removeEventListener("mousedown", away);
  }, [open]);

  const close = (refocus: boolean) => {
    setOpen(false);
    if (refocus) button.current?.focus();
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      close(true);
    } else if (e.key === "Tab") setOpen(false);
    else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const els = [...(list.current?.querySelectorAll<HTMLElement>("[role=menuitem]:not([disabled])") ?? [])];
      const at = els.indexOf(document.activeElement as HTMLElement);
      els[(at + (e.key === "ArrowDown" ? 1 : -1) + els.length) % els.length]?.focus();
    }
  };

  return (
    <span className={cx("menu", className)}>
      {trigger({ onClick: () => setOpen((o) => !o), "aria-haspopup": "menu", "aria-expanded": open, ref: (el) => (button.current = el) }, open)}
      {open && (
        <div ref={list} className="menu-list" role="menu" aria-label={label} style={{ ...pos, width: Math.min(WIDTH, window.innerWidth - 16) }} onKeyDown={onKeyDown} onMouseDown={(e) => e.stopPropagation()}>
          {items.map((it) => (
            <button
              key={it.key}
              type="button"
              role="menuitem"
              disabled={it.disabled}
              className={cx("menu-item", it.tone === "danger" && "is-danger", it.selected && "is-selected", it.separated && "has-sep")}
              onClick={() => {
                close(false);
                it.onSelect();
              }}
            >
              {it.icon && <i className={`ph ph-${it.icon}`} aria-hidden="true" />}
              <span className="menu-item-label">{it.label}</span>
              {it.trailing}
              {it.selected && <i className="ph ph-check menu-check" aria-hidden="true" />}
            </button>
          ))}
        </div>
      )}
    </span>
  );
}
