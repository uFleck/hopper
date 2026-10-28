/* Turns a key press into the accelerator string the backend registers, e.g. "Ctrl+Alt+H". */

export type ComboResult = { ok: true; combo: string } | { ok: false; reason: "modifier-only" | "needs-modifier" | "unsupported" };

interface KeyLike {
  code: string;
  ctrlKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
  metaKey: boolean;
}

const MODIFIER_CODES = /^(Control|Alt|Shift|Meta|OS)(Left|Right)?$/;
const NAMED: Record<string, string> = {
  Space: "Space", Enter: "Enter", Tab: "Tab", Backspace: "Backspace", Delete: "Delete", Insert: "Insert",
  Home: "Home", End: "End", PageUp: "PageUp", PageDown: "PageDown",
  ArrowUp: "Up", ArrowDown: "Down", ArrowLeft: "Left", ArrowRight: "Right",
  Minus: "-", Equal: "=", Comma: ",", Period: ".", Slash: "/", Semicolon: ";", Quote: "'", Backquote: "`", BracketLeft: "[", BracketRight: "]", Backslash: "\\",
};

export function keyName(code: string): string | null {
  const letter = /^Key([A-Z])$/.exec(code);
  if (letter) return letter[1];
  const digit = /^Digit(\d)$/.exec(code);
  if (digit) return digit[1];
  if (/^F([1-9]|1\d|2[0-4])$/.test(code)) return code;
  return NAMED[code] ?? null;
}

export function comboFromEvent(e: KeyLike): ComboResult {
  if (MODIFIER_CODES.test(e.code)) return { ok: false, reason: "modifier-only" };
  const key = keyName(e.code);
  if (!key) return { ok: false, reason: "unsupported" };
  // Shift alone is just typing; a global shortcut needs Ctrl, Alt or Win as well.
  if (!e.ctrlKey && !e.altKey && !e.metaKey) return { ok: false, reason: "needs-modifier" };
  const parts = [e.ctrlKey && "Ctrl", e.altKey && "Alt", e.shiftKey && "Shift", e.metaKey && "Super", key].filter(Boolean) as string[];
  return { ok: true, combo: parts.join("+") };
}

export const comboParts = (combo: string): string[] => combo.split("+").filter(Boolean);

export const RECORD_HINTS: Record<Exclude<ComboResult, { ok: true }>["reason"], string> = {
  "modifier-only": "Now press the key to go with it.",
  "needs-modifier": "Include Ctrl, Alt or Win so typing is not hijacked.",
  unsupported: "That key cannot be used. Try a letter, digit or F-key.",
};
