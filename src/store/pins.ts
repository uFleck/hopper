import { useSyncExternalStore } from "react";

const KEY = "hopper.pinned";
const listeners = new Set<() => void>();

const read = (): string[] => {
  try {
    const v: unknown = JSON.parse(localStorage.getItem(KEY) ?? "[]");
    return Array.isArray(v) ? v.filter((x): x is string => typeof x === "string") : [];
  } catch {
    return [];
  }
};

let pins = read();

function write(next: string[]) {
  pins = next;
  try {
    localStorage.setItem(KEY, JSON.stringify(next));
  } catch {
    /* storage unavailable: the pins last for this session */
  }
  listeners.forEach((l) => l());
}

const subscribe = (l: () => void) => {
  listeners.add(l);
  return () => void listeners.delete(l);
};

/** Pinned request ids, oldest first. Shown as lamps on the home screen. */
export const usePins = () => useSyncExternalStore(subscribe, () => pins);

export const togglePin = (id: string) => write(pins.includes(id) ? pins.filter((p) => p !== id) : [...pins, id]);
