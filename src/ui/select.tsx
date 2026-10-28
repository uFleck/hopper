import { Children, Fragment, isValidElement, useCallback, useEffect, useId, useLayoutEffect, useRef, useState, type KeyboardEvent, type ReactElement, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { cx } from "./primitives";

interface Choice {
  value: string;
  label: ReactNode;
  disabled: boolean;
}

/** Reads `<option>` children (also inside fragments and arrays), so call sites keep the native shape. */
function choices(children: ReactNode, out: Choice[] = []): Choice[] {
  Children.forEach(children, (child) => {
    if (!isValidElement(child)) return;
    const el = child as ReactElement<{ value?: string | number; disabled?: boolean; children?: ReactNode }>;
    if (el.type === Fragment) choices(el.props.children, out);
    else if (el.type === "option") {
      const label = el.props.children;
      out.push({ value: String(el.props.value ?? (typeof label === "string" ? label : "")), label, disabled: !!el.props.disabled });
    }
  });
  return out;
}

export interface SelectProps {
  value: string;
  /** Same shape as the native select, so `(e) => set(e.target.value)` keeps working. */
  onChange?: (e: { target: { value: string } }) => void;
  invalid?: boolean;
  disabled?: boolean;
  className?: string;
  id?: string;
  "aria-label"?: string;
  children: ReactNode;
}

interface Pos {
  left: number;
  minWidth: number;
  top?: number;
  bottom?: number;
  maxHeight: number;
}

const GAP = 6;
const EDGE = 8;

/** A themed dropdown: a button that opens a listbox under (or over) it. Focus stays on the button. */
export function Select({ value, onChange, invalid, disabled, className, id, children, "aria-label": ariaLabel }: SelectProps) {
  const options = choices(children);
  const selected = options.findIndex((o) => o.value === value);
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  const [pos, setPos] = useState<Pos | null>(null);
  const btn = useRef<HTMLButtonElement>(null);
  const list = useRef<HTMLDivElement>(null);
  const listId = useId();

  const place = useCallback(() => {
    const r = btn.current?.getBoundingClientRect();
    if (!r) return;
    const below = window.innerHeight - r.bottom - GAP - EDGE;
    const above = r.top - GAP - EDGE;
    const flip = below < 160 && above > below;
    const room = flip ? above : below;
    setPos({
      left: Math.max(EDGE, Math.min(r.left, window.innerWidth - r.width - EDGE)),
      minWidth: r.width,
      maxHeight: Math.max(120, Math.min(320, room)),
      ...(flip ? { bottom: window.innerHeight - r.top + GAP } : { top: r.bottom + GAP }),
    });
  }, []);

  useLayoutEffect(() => {
    if (open) place();
  }, [open, place, options.length]);

  useEffect(() => {
    if (!open) return;
    const away = (e: MouseEvent) => {
      const t = e.target as Node;
      if (!btn.current?.contains(t) && !list.current?.contains(t)) setOpen(false);
    };
    const close = (e: Event) => {
      if (!list.current?.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener("mousedown", away);
    window.addEventListener("resize", close);
    window.addEventListener("scroll", close, true);
    return () => {
      document.removeEventListener("mousedown", away);
      window.removeEventListener("resize", close);
      window.removeEventListener("scroll", close, true);
    };
  }, [open]);

  useEffect(() => {
    if (open) list.current?.querySelector<HTMLElement>(`[data-i="${active}"]`)?.scrollIntoView({ block: "nearest" });
  }, [open, active, pos]);

  const show = () => {
    if (disabled) return;
    setActive(Math.max(0, selected));
    setOpen(true);
  };

  const pick = (i: number) => {
    const o = options[i];
    if (!o || o.disabled) return;
    setOpen(false);
    if (o.value !== value) onChange?.({ target: { value: o.value } });
    btn.current?.focus();
  };

  const step = (from: number, dir: 1 | -1) => {
    for (let n = 1; n <= options.length; n++) {
      const i = (from + dir * n + options.length * n) % options.length;
      if (!options[i].disabled) return i;
    }
    return from;
  };

  const onKey = (e: KeyboardEvent) => {
    if (disabled) return;
    if (e.key === "Escape" && open) {
      e.preventDefault();
      e.stopPropagation();
      setOpen(false);
    } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      if (!open) show();
      else setActive((a) => step(a, e.key === "ArrowDown" ? 1 : -1));
    } else if (e.key === "Home" || e.key === "End") {
      if (!open) return;
      e.preventDefault();
      setActive(e.key === "Home" ? step(-1, 1) : step(options.length, -1));
    } else if ((e.key === "Enter" || e.key === " ") && open) {
      e.preventDefault();
      pick(active);
    } else if (e.key === "Tab" && open) {
      setOpen(false);
    } else if (e.key.length === 1 && /\S/.test(e.key)) {
      const from = open ? active : Math.max(0, selected);
      const at = options.findIndex((o, i) => i > from && !o.disabled && typeof o.label === "string" && o.label.toLowerCase().startsWith(e.key.toLowerCase()));
      if (at >= 0) (open ? setActive : pick)(at);
    }
  };

  const current = selected >= 0 ? options[selected] : undefined;

  return (
    <span className={cx("ui-select", open && "is-open", className)}>
      <button
        ref={btn}
        id={id}
        type="button"
        className="ui-select-btn"
        role="combobox"
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-controls={open ? listId : undefined}
        aria-activedescendant={open ? `${listId}-${active}` : undefined}
        aria-label={ariaLabel}
        aria-invalid={invalid || undefined}
        disabled={disabled}
        onClick={() => (open ? setOpen(false) : show())}
        onKeyDown={onKey}
      >
        <span className="ui-select-value">{current ? current.label : "Select…"}</span>
        <i className="ph ph-caret-down ui-select-caret" aria-hidden="true" />
      </button>
      {open &&
        pos &&
        createPortal(
          <div ref={list} id={listId} role="listbox" className="ui-select-list" style={{ ...pos }}>
            {options.map((o, i) => (
              <div
                key={`${o.value}-${i}`}
                id={`${listId}-${i}`}
                data-i={i}
                role="option"
                aria-selected={i === selected}
                aria-disabled={o.disabled || undefined}
                className={cx("ui-select-opt", i === selected && "is-selected", i === active && "is-active", o.disabled && "is-disabled")}
                onMouseEnter={() => !o.disabled && setActive(i)}
                onMouseDown={(e) => e.preventDefault()}
                onClick={() => pick(i)}
              >
                <span className="ui-select-opt-label">{o.label}</span>
                {i === selected && <i className="ph ph-check ui-select-check" aria-hidden="true" />}
              </div>
            ))}
          </div>,
          document.body,
        )}
    </span>
  );
}
