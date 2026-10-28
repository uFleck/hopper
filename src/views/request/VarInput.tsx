import { useId, useRef, useState, type KeyboardEvent, type MouseEvent, type Ref } from "react";
import { promptAddVariable } from "../../store";
import { Icon, cx, useHoverCard } from "../../ui";
import { unresolvedNames, variableAt, type VarInfo } from "./logic";
import { VarPopover } from "./VarPopover";

/** Index of the character under `clientX` in a single-line input, measured with the input's own font. -1 past the end of the text. */
function charIndexAt(input: HTMLInputElement, clientX: number): number {
  const ctx = document.createElement("canvas").getContext("2d");
  if (!ctx) return -1;
  const cs = getComputedStyle(input);
  ctx.font = `${cs.fontStyle} ${cs.fontWeight} ${cs.fontSize} ${cs.fontFamily}`;
  const x = clientX - input.getBoundingClientRect().left - parseFloat(cs.borderLeftWidth) - parseFloat(cs.paddingLeft) + input.scrollLeft;
  if (x < 0 || x > ctx.measureText(input.value).width) return -1;
  for (let i = 0; i < input.value.length; i++) if (ctx.measureText(input.value.slice(0, i + 1)).width > x) return i;
  return -1;
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
}

/** A one-line input with `{{` autocomplete from the environment; unresolved names turn the border red and are listed in the tooltip. */
export function VarInput({ value, onChange, info, placeholder, ariaLabel, disabled, mono = true, className, ref, onKeyDown, data }: VarInputProps) {
  const el = useRef<HTMLInputElement | null>(null);
  const listId = useId();
  const [caret, setCaret] = useState(0);
  const [focused, setFocused] = useState(false);
  const [dismissed, setDismissed] = useState(false);
  const [active, setActive] = useState(0);

  const open = /\{\{\s*([\w.-]*)$/.exec(value.slice(0, caret));
  const word = open?.[1] ?? "";
  const names = [...info.known];
  const matches = open && focused && !dismissed ? names.filter((n) => n.toLowerCase().includes(word.toLowerCase())).slice(0, 8) : [];
  const bad = unresolvedNames(value, info);

  // The variable under the pointer; its card stays while the pointer or focus is in it.
  const hover = useHoverCard<string>();
  const onMove = (e: MouseEvent<HTMLInputElement>) => {
    const name = variableAt(value, charIndexAt(e.currentTarget, e.clientX));
    if (name) hover.showSoon(name, !!hover.value);
    else hover.closeSoon();
  };

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
        accept(matches[Math.min(active, matches.length - 1)]);
        return;
      }
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        setDismissed(true);
        return;
      }
    }
    onKeyDown?.(e);
  };

  return (
    <span className={cx("rv-varinput", className)}>
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
        title={bad.length ? `Not defined in the active environment: ${bad.join(", ")}` : undefined}
        value={value}
        placeholder={placeholder}
        disabled={disabled}
        spellCheck={false}
        onMouseMove={onMove}
        onMouseLeave={() => hover.value && hover.closeSoon()}
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
      {bad.length > 0 && !disabled && !focused && (
        <button type="button" className="rv-varadd" aria-label={`Add ${bad[0]}`} title={`${bad[0]} has no value. Add it.`} onClick={() => promptAddVariable(bad[0])}>
          <Icon name="plus" />
        </button>
      )}
      {hover.value && matches.length === 0 && (
        <div {...hover.cardProps} className="rv-varpop">
          <VarPopover key={hover.value} name={hover.value} info={info} />
        </div>
      )}
      {matches.length > 0 && (
        <ul className="rv-suggest" id={listId} role="listbox">
          {matches.map((n, i) => (
            <li key={n} role="option" aria-selected={i === active} className={cx(i === active && "is-on")} onMouseDown={(e) => (e.preventDefault(), accept(n))}>
              <span>{n}</span>
              <span className="rv-suggest-detail">{info.secrets.has(n) ? "secret" : info.values.get(n)}</span>
            </li>
          ))}
        </ul>
      )}
    </span>
  );
}
