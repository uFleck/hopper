import { useSyncExternalStore } from "react";

export interface Store<T> {
  getState: () => T;
  /** Merges a partial state, or the partial returned by an updater that sees the current state. */
  setState: (patch: Partial<T> | ((s: T) => Partial<T>)) => void;
  /** Replaces the whole state (used by tests to reset). */
  replaceState: (next: T) => void;
  subscribe: (listener: () => void) => () => void;
}

/** A tiny typed store: the same idea as Spawner's, without the domain. */
export function createStore<T extends object>(initial: T): Store<T> {
  let state = initial;
  const listeners = new Set<() => void>();
  const emit = () => listeners.forEach((l) => l());
  return {
    getState: () => state,
    setState(patch) {
      const part = typeof patch === "function" ? patch(state) : patch;
      const keys = Object.keys(part) as (keyof T)[];
      if (keys.every((k) => Object.is(state[k], part[k]))) return;
      state = { ...state, ...part };
      emit();
    },
    replaceState(next) {
      state = next;
      emit();
    },
    subscribe(listener) {
      listeners.add(listener);
      return () => void listeners.delete(listener);
    },
  };
}

/** Subscribes a component to a slice of a store. The selector must return a stable value (a field, not a fresh object). */
export function useStoreSelector<T extends object, S>(store: Store<T>, selector: (s: T) => S): S {
  return useSyncExternalStore(store.subscribe, () => selector(store.getState()));
}
