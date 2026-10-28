import { useRef, type KeyboardEvent } from "react";
import type { KeyValue } from "../../ipc";
import { IconButton } from "../../ui";
import type { VarInfo } from "./logic";
import { VarInput } from "./VarInput";

export interface KeyValueTableProps {
  rows: KeyValue[];
  onChange: (rows: KeyValue[]) => void;
  info: VarInfo;
  /** Names the rows, for accessible labels: "header", "query parameter". */
  noun: string;
  disabled?: boolean;
  namePlaceholder?: string;
  valuePlaceholder?: string;
}

/** Rows of enabled / name / value. A blank row at the end creates the next one as you type; Enter in a value jumps to the next row. */
export function KeyValueTable({ rows, onChange, info, noun, disabled, namePlaceholder = "name", valuePlaceholder = "value" }: KeyValueTableProps) {
  const table = useRef<HTMLDivElement>(null);
  const shown = disabled ? rows : [...rows, { name: "", value: "", enabled: true }];

  const set = (i: number, patch: Partial<KeyValue>) => {
    if (i >= rows.length) onChange([...rows, { name: "", value: "", enabled: true, ...patch }]);
    else onChange(rows.map((r, j) => (j === i ? { ...r, ...patch } : r)));
  };

  const focusCell = (row: number, col: "name" | "value") =>
    requestAnimationFrame(() => table.current?.querySelector<HTMLInputElement>(`[data-row="${row}"][data-col="${col}"]`)?.focus());

  const keys = (i: number, col: "name" | "value") => (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "Enter" && col === "value" && !e.defaultPrevented) {
      e.preventDefault();
      focusCell(i + 1, "name");
    } else if (e.key === "Backspace" && col === "name" && !shown[i].name && !shown[i].value && i < rows.length) {
      e.preventDefault();
      onChange(rows.filter((_, j) => j !== i));
      focusCell(Math.max(0, i - 1), "value");
    }
  };

  return (
    <div className="rv-kv" ref={table} role="group" aria-label={`${noun}s`}>
      {shown.map((r, i) => {
        const ghost = i >= rows.length;
        return (
          <div key={i} className={`rv-kv-row${ghost ? " is-ghost" : ""}${r.enabled ? "" : " is-off"}`}>
            <input type="checkbox" className="rv-check" checked={r.enabled} disabled={disabled || ghost} aria-label={`Enable ${noun} ${r.name || i + 1}`} onChange={(e) => set(i, { enabled: e.target.checked })} />
            <VarInput value={r.name} info={info} ariaLabel={`${noun} ${i + 1} name`} placeholder={namePlaceholder} disabled={disabled} onChange={(v) => set(i, { name: v })} onKeyDown={keys(i, "name")} data={{ "data-row": String(i), "data-col": "name" }} />
            <VarInput value={r.value} info={info} ariaLabel={`${noun} ${i + 1} value`} placeholder={valuePlaceholder} disabled={disabled} onChange={(v) => set(i, { value: v })} onKeyDown={keys(i, "value")} data={{ "data-row": String(i), "data-col": "value" }} />
            {ghost || disabled ? <span className="rv-kv-gap" /> : <IconButton icon="x" size="sm" label={`Remove ${noun} ${r.name || i + 1}`} onClick={() => onChange(rows.filter((_, j) => j !== i))} />}
          </div>
        );
      })}
    </div>
  );
}
