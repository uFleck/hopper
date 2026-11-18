import type { AgentAccess, GrantScope } from "../../ipc";

/** What the user picks for a saved request: the access plus, for Auto, how much of the request it binds. */
export type AgentMode = "ask" | "auto" | "auto_params" | "auto_any" | "blocked";

export interface ModeMeta {
  label: string;
  short: string;
  icon: string;
  tone: "neutral" | "ok" | "warn" | "err";
  hint: string;
  /** Plain-language prompt-injection warning for the opt-in modes. */
  risk?: string;
}

export const MODES: Record<AgentMode, ModeMeta> = {
  ask: { label: "Ask", short: "Ask", icon: "hand-palm", tone: "neutral", hint: "The agent asks you before each send" },
  auto: { label: "Auto", short: "Auto", icon: "lightning", tone: "ok", hint: "The agent can send it as saved without asking. Any edit pauses this" },
  auto_params: {
    label: "Auto: parameters may change",
    short: "Auto · params",
    icon: "lightning",
    tone: "warn",
    hint: "The agent can change path and query values and send without asking. Other edits pause this",
    risk: "Text an agent reads, such as a web page, a file or an API response, can steer it. In this mode such text could make it send this request with values you never saw.",
  },
  auto_any: {
    label: "Auto: trusted, any edit",
    short: "Auto · any edit",
    icon: "lightning",
    tone: "warn",
    hint: "The agent can rewrite this request any way and send it without asking, to the same host",
    risk: "Text an agent reads, such as a web page, a file or an API response, can steer it. In this mode such text could make it rewrite this request (method, headers, body) and send it without asking you.",
  },
  blocked: { label: "Blocked", short: "Blocked", icon: "prohibit", tone: "err", hint: "The agent cannot send this request" },
};

export const MODE_ORDER: AgentMode[] = ["ask", "auto", "auto_params", "auto_any", "blocked"];

export function modeOf(access: AgentAccess, scope: GrantScope): AgentMode {
  if (access !== "auto") return access;
  return scope === "params" ? "auto_params" : scope === "any" ? "auto_any" : "auto";
}

export function accessOf(mode: AgentMode): { access: AgentAccess; scope: GrantScope } {
  switch (mode) {
    case "auto_params":
      return { access: "auto", scope: "params" };
    case "auto_any":
      return { access: "auto", scope: "any" };
    default:
      return { access: mode, scope: "exact" };
  }
}
