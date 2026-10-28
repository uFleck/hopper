import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { inTauri } from "../ipc";

export type PanelMode = "hidden" | "tab" | "compact" | "open";

/** The mode, and the screen edge Hopper is stuck to. */
export interface PanelModeInfo {
  mode: PanelMode;
  right: boolean;
}

/** User gestures the page may report. The backend decides what they do; the page can never take focus on its own. */
export type PanelGesture = "dismiss" | "clicked" | "hover-enter" | "hover-leave" | "toggle-pin" | "open";

export function panelEvent(name: PanelGesture): void {
  if (inTauri()) void tauriInvoke("panel_event", { name });
}

/** Starts dragging Hopper off its edge; it snaps to the nearest one on release. Resolves when the button is released, with whether it moved; a press that did not move is a click. */
export async function panelDrag(): Promise<boolean> {
  return inTauri() ? tauriInvoke<boolean>("panel_drag") : false;
}

/** The mode the window is in now. In a browser, `?mode=tab|compact` previews the floating views and the default is the open panel. */
export async function panelMode(): Promise<PanelModeInfo> {
  if (inTauri()) {
    const info = await tauriInvoke<PanelModeInfo | null>("panel_mode");
    if (info) return info;
  }
  const asked = new URLSearchParams(window.location.search).get("mode");
  const mode: PanelMode = asked === "tab" || asked === "compact" ? asked : "open";
  return { mode, right: true };
}

/** Subscribes to the backend's panel events; a no-op in the browser. Returns the unsubscribe function. */
export function onPanel(handlers: { summon?: () => void; collapsing?: () => void; mode?: (info: PanelModeInfo) => void }): () => void {
  if (!inTauri()) return () => {};
  const subs = [
    listen("panel://summon", () => handlers.summon?.()),
    listen("panel://collapsing", () => handlers.collapsing?.()),
    listen<PanelModeInfo>("panel://mode", (e) => handlers.mode?.(e.payload)),
  ];
  return () => subs.forEach((p) => void p.then((off) => off()));
}
