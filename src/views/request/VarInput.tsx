import { useEffect, useId, useRef, useState, type KeyboardEvent, type MouseEvent, type Ref } from "react";
import { createPortal } from "react-dom";
import { Icon, cx, useCardPlacement, useHoverCard, type CardAnchor } from "../../ui";
import { renameVariable, suggest, unresolvedNames, variablesIn, variableTokenAt, type VarInfo } from "./logic";
import { VarPopover } from "./VarPopover";

/** The input's own font and the x offset of its text, for measuring where characters sit. */
function measurer(input: HTMLInputElement) {
  const ctx = document.createElement("canvas").getContext("2d");
  if (!ctx) return null;
  const cs = getComputedStyle(input);
  ctx.font = `${cs.fontStyle} ${cs.fontWeight} ${cs.fontSize} ${cs.fontFamily}`;
  const rect = input.getBoundingClientRect();
  const origin = rect.left + parseFloat(cs.borderLeftWidth) + parseFloat(cs.paddingLeft) - input.scrollLeft;
  return { width: (text: string) => ctx.measureText(text).width, origin, rect };
}

/** Index of the character under `clientX` in a single-line input. -1 past the end of the text. */
function charIndexAt(input: HTMLInputElement, clientX: number): number {
  const m = measurer(input);
  if (!m) return -1;
  const x = clientX - m.origin;
  if (x < 0 || x > m.width(input.value)) return -1;
  for (let i = 0; i < input.value.length; i++) if (m.width(input.value.slice(0, i + 1)) > x) return i;
  return -1;
}

/** Where a card for the token starting at `from` hangs: under the token's first letter, or above the field when there is no room. */
function anchorAt(input: HTMLInputElement, from: number): CardAnchor {
  const m = measurer(input);
  if (!m) return { left: 0, top: 0, above: 0 };
  const left = Math.max(m.rect.left, Math.min(m.origin + m.width(input.value.slice(0, from)), m.rect.right - 24));
  return { left, top: m.rect.bottom + 4, above: m.rect.top - 4 };
}

interface VarHover extends CardAnchor {
  name: string;
  from: number;
}

export interface VarInputProps {
  value: string;
  onChange: (value: string) => void;
  info: VarInfo;
  placeholder?: string;
  ariaLabel: string;
  disabled?: boolean;
  mono?: boolean;
  className?: string;
  ref?: Ref<HTMLInputElement>;
  /** `data-*` attributes for the input, e.g. to find it again. */
  data?: Record<string, string>;
  onKeyDown?: (e: KeyboardEvent<HTMLInputElement>) => void;
  /** False where saving a variable from the card would fight the edit in progress, e.g. the collection's own variable rows. */
  canDefine?: boolean;
}

/**
 * A one-line input with `{{` autocomplete over every variable the request can see (scope and value shown), so a name is picked, not typed.
 * A name nothing defines turns the border red; the warning button (and hovering the name) opens a card to fix a typo or define it.
 */
export function VarInput({ value, onChange, info, placeholder, ariaLabel, disabled, mono = true, className, ref, onKeyDown, data, canDefine = true }: VarInputProps) {
  const el = useRef<HTMLInputElement | null>(null);
  const root = useRef<HTMLSpanElement>(null);
  const listId = useId();
  const [caret, setCaret] = useState(0);
  const [focused, setFocused] = useState(false);
  const [dismissed, setDismissed] = useState(false);
  const [active, setActive] = useState(0);

  const open = /\{\{\s*([\w.$-]*)$/.exec(value.slice(0, caret));
  const word = open?.[1] ?? "";
  const listing = !!open && focused && !dismissed;
  const matches = listing ? suggest(info, word).slice(0, 50) : [];
  const bad = unresolvedNames(value, info);

  // The variable under the pointer, or pinned by the warning button; its card stays while the pointer or focus is in it.
  const hover = useHoverCard<VarHover>(450, 120);
  useCardPlacement(hover.cardProps.ref, hover.value);
  const pinned = useRef(false);
  const onMove = (e: MouseEvent<HTMLInputElement>) => {
    const token = variableTokenAt(value, charIndexAt(e.currentTarget, e.clientX));
    if (!token) return hover.closeSoon();
    if (hover.value?.from === token.from && hover.value.name === token.name) return hover.keep();
    pinned.current = false;
    hover.showSoon({ name: token.name, from: token.from, ...anchorAt(e.currentTarget, token.from) }, !!hover.value);
  };
  const pin = (name: string) => {
    const input = el.current;
    const from = input ? value.search(new RegExp(`\\{\\{\\s*${name.replace(/[.*+?^${}()|[\]\\-]/g, "\\$&")}\\s*\\}\\}`)) : -1;
    if (!input || from < 0) return;
    pinned.current = true;
    hover.show({ name, from, ...anchorAt(input, from) });
  };
  // A card opened by click has no pointer to leave, so it closes on a press elsewhere.
  const card = hover.cardProps.ref;
  const shown = hover.value !== null;
  const { close } = hover;
  useEffect(() => {
    if (!shown) return;
    const away = (e: Event) => {
      const t = e.target as Node;
      if (!card.current?.contains(t) && !root.current?.contains(t)) close();
    };
    document.addEventListener("mousedown", away);
    return () => document.removeEventListener("mousedown", away);
  }, [shown, card, close]);
  // The token the card was opened for is gone (renamed or deleted): drop the card.
  const hoveredName = hover.value?.name;
  useEffect(() => {
    if (hoveredName && !variablesIn(value).includes(hoveredName)) close();
  }, [value, hoveredName, close]);

  const accept = (name: string) => {
    const from = caret - word.length;
    const closed = value.slice(caret, caret + 2) === "}}";
    const insert = closed ? name : `${name}}}`;
    const next = value.slice(0, from) + insert + value.slice(caret);
    onChange(next);
    const pos = from + insert.length + (closed ? 2 : 0);
    requestAnimationFrame(() => {
      el.current?.setSelectionRange(pos, pos);
      setCaret(pos);
    });
    setDismissed(true);
  };

  const keys = (e: KeyboardEvent<HTMLInputElement>) => {
    if (matches.length) {
      if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        setActive((a) => (a + (e.key === "ArrowDown" ? 1 : -1) + matches.length) % matches.length);
        return;
      }
      if (e.key === "Enter" || e.key === "Tab") {
        e.preventDefault();
        accept(matches[Math.min(active, matches.length - 1)].name);
        return;
      }
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        setDismissed(true);
        return;
      }
    }
    if (e.key === "Escape" && hover.value) {
      e.stopPropagation();
      hover.close();
      return;
    }
    onKeyDown?.(e);
  };

  return (
    <span ref={root} className={cx("rv-varinput", className)}>
      <input
        ref={(n) => {
          el.current = n;
          if (typeof ref === "function") ref(n);
          else if (ref) ref.current = n;
        }}
        className={cx("ui-input", mono && "ui-input--mono", bad.length > 0 && "is-invalid")}
        aria-invalid={bad.length > 0 || undefined}
        aria-label={ariaLabel}
        aria-autocomplete="list"
        aria-controls={matches.length ? listId : undefined}
        {...data}
        value={value}
        placeholder={placeholder}
        disabled={disabled}
        spellCheck={false}
        onMouseMove={onMove}
        onMouseLeave={() => hover.value && !pinned.current && hover.closeSoon()}
        onChange={(e) => {
          setDismissed(false);
          setActive(0);
          setCaret(e.target.selectionStart ?? e.target.value.length);
          onChange(e.target.value);
        }}
        onSelect={(e) => setCaret(e.currentTarget.selectionStart ?? 0)}
        onFocus={() => setFocused(true)}
        onBlur={() => setFocused(false)}
        onKeyDown={keys}
      />
      {bad.length > 0 && !disabled && (
        <button
          type="button"
          className="rv-varadd"
          aria-label={`${bad[0]} is not defined. Fix it`}
          data-tip={`{{${bad[0]}}} is not defined${bad.length > 1 ? ` (and ${bad.length - 1} more)` : ""}. Click to fix it.`}
          onMouseDown={(e) => e.preventDefault()}
          onClick={() => pin(bad[0])}
        >
          <Icon name="warning-circle" />
        </button>
      )}
      {hover.value &&
        !listing &&
        createPortal(
          <div {...hover.cardProps} className="vp-float" style={{ left: hover.value.left, top: hover.value.top }}>
            <VarPopover
              key={hover.value.name}
              name={hover.value.name}
              info={info}
              canDefine={canDefine}
              rename={(to) => {
                onChange(renameVariable(value, hover.value!.name, to));
                hover.close();
              }}
            />
          </div>,
          document.body,
        )}
      {listing && (
        <ul className="rv-suggest" id={listId} role="listbox" aria-label="Variables">
          {matches.map((s, i) => (
            <li
              key={s.name}
              role="option"
              aria-selected={i === active}
              className={cx(i === active && "is-on")}
              ref={(n) => {
                if (i === active) n?.scrollIntoView({ block: "nearest" });
              }}
              onMouseDown={(e) => (e.preventDefault(), accept(s.name))}
              onMouseEnter={() => setActive(i)}
            >
              <span className="rv-suggest-name">{s.name}</span>
              <span className={cx("rv-suggest-kind", `vp-kind--${s.kind}`)}>{s.kind}</span>
              <span className="rv-suggest-detail">{s.kind === "secret" || s.kind === "builtin" ? "" : s.detail || "empty"}</span>
            </li>
          ))}
          {matches.length === 0 && (
            <li className="rv-suggest-none" role="presentation">
              {word ? <>No variable matches <code>{word}</code>. Finish the name and click the warning to define it.</> : "No variables yet."}
            </li>
          )}
        </ul>
      )}
    </span>
  );
}
