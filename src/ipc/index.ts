import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { listen as tauriListen } from "@tauri-apps/api/event";
import type { CommandMap, CommandName, EventMap, EventName, HopperError } from "./types";
import { bus } from "./bus";

export * from "./types";

/** True inside the Tauri webview. In a plain browser the dev mock answers instead. */
export const inTauri = () => typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

/** Turns whatever a command rejected with into a `HopperError`. */
export function toHopperError(e: unknown): HopperError {
  if (typeof e === "string") return { code: "internal", message: e };
  if (e && typeof e === "object" && "code" in e && "message" in e) {
    const o = e as Record<string, unknown>;
    if (typeof o.code === "string" && typeof o.message === "string") return e as HopperError;
  }
  if (e instanceof Error) return { code: "internal", message: e.message };
  return { code: "internal", message: String(e) };
}

export function isHopperError(e: unknown, code?: string): e is HopperError {
  return !!e && typeof e === "object" && "code" in e && "message" in e && (code === undefined || (e as HopperError).code === code);
}

/** Calls a backend command. Outside Tauri, in `pnpm dev`, the call goes to the mock backend. Rejects with a `HopperError`. */
export async function invoke<K extends CommandName>(cmd: K, args?: CommandMap[K]["args"]): Promise<CommandMap[K]["result"]> {
  try {
    if (inTauri()) return await tauriInvoke<CommandMap[K]["result"]>(cmd, args as Record<string, unknown> | undefined);
    if (!import.meta.env.DEV) throw new Error(`Hopper: no backend for ${cmd}`);
    const { mockInvoke } = await import("./dev/mockBackend");
    return await mockInvoke(cmd, args);
  } catch (e) {
    throw toHopperError(e);
  }
}

/** Subscribes to a backend event. Returns the unsubscribe function. In a plain browser the mock emits on an in-memory bus. */
export function listen<K extends EventName>(event: K, handler: (payload: EventMap[K]) => void): () => void {
  if (inTauri()) {
    const sub = tauriListen<EventMap[K]>(event, (e) => handler(e.payload));
    return () => void sub.then((off) => off());
  }
  return bus.on(event, handler);
}
