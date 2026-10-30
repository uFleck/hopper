import { useEffect, useLayoutEffect, useRef, useState, type CSSProperties, type ReactNode, type RefObject } from "react";
import { cx } from "../../ui/primitives";

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

export interface MenuListProps {
  items: MenuItem[];
  label: string;
  /** Position and size. The caller places the list; `MenuList` only renders it. */
  style: CSSProperties;
  listRef: RefObject<HTMLDivElement | null>;
  /** Esc calls it with `true` (refocus the opener); Tab and a picked item call it with `false`. */
  onClose: (refocus: boolean) => void;
}

const WIDTH = 240;

/** The list of actions: focuses the first item on mount, arrow keys move, Esc and Tab close. Fixed-positioned by the caller through `style`. */
export function MenuList({ items, label, style, listRef, onClose }: MenuListProps) {
  useEffect(() => {
    listRef.current?.querySelector<HTMLElement>("[role=menuitem]:not([disabled])")?.focus();
  }, [listRef]);

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      onClose(true);
    } else if (e.key === "Tab") onClose(false);
    else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const els = [...(listRef.current?.querySelectorAll<HTMLElement>("[role=menuitem]:not([disabled])") ?? [])];
      const at = els.indexOf(document.activeElement as HTMLElement);
      els[(at + (e.key === "ArrowDown" ? 1 : -1) + els.length) % els.length]?.focus();
    }
  };

  return (
    <div ref={listRef} className="menu-list" role="menu" aria-label={label} style={style} onKeyDown={onKeyDown} onMouseDown={(e) => e.stopPropagation()}>
      {items.map((it) => (
        <button
          key={it.key}
          type="button"
          role="menuitem"
          disabled={it.disabled}
          className={cx("menu-item", it.tone === "danger" && "is-danger", it.selected && "is-selected", it.separated && "has-sep")}
          onClick={() => {
            onClose(false);
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
  );
}

/** A popover list of actions. Fixed-positioned from its trigger so scroll containers do not clip it. Esc closes it without dismissing the panel. */
export function Menu({ trigger, items, label, align = "end", className }: MenuProps) {
  const [open, setOpen] = useState(false);
  const [pos, setPos] = useState<{ top?: number; bottom?: number; left: number; maxHeight: number }>({ left: 0, maxHeight: 320 });
  const button = useRef<HTMLButtonElement | null>(null);
  const list = useRef<HTMLDivElement | null>(null);

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

  return (
    <span className={cx("menu", className)}>
      {trigger({ onClick: () => setOpen((o) => !o), "aria-haspopup": "menu", "aria-expanded": open, ref: (el) => (button.current = el) }, open)}
      {open && <MenuList items={items} label={label} listRef={list} style={{ ...pos, width: Math.min(WIDTH, window.innerWidth - 16) }} onClose={close} />}
    </span>
  );
}
