import type { AgentAccess, GrantScope } from "./ipc";

/** What the user picks for one saved request: an access, and for Auto how far the grant reaches. */
export type AgentMode = "ask" | "auto" | "auto-params" | "auto-any" | "blocked";

export interface AgentModeMeta {
  access: AgentAccess;
  scope: GrantScope;
  label: string;
  short: string;
  icon: string;
  tone: string;
  hint: string;
  /** Plain-language risk, shown wherever the mode is offered. */
  warning?: string;
}

const INJECTION =
  "Text an agent reads (a web page, an issue, an API response) can tell it what to send. Use this only for requests whose data you are fine with any value reaching.";

export const AGENT_MODES: Record<AgentMode, AgentModeMeta> = {
  ask: { access: "ask", scope: "exact", label: "Ask", short: "Ask", icon: "hand-palm", tone: "tone-neutral", hint: "The agent asks you before each send" },
  auto: { access: "auto", scope: "exact", label: "Auto", short: "Auto", icon: "lightning", tone: "tone-ok", hint: "The agent can send it as it is now; any edit asks again" },
  "auto-params": {
    access: "auto",
    scope: "params",
    label: "Auto: parameters may change",
    short: "Auto: params",
    icon: "lightning",
    tone: "tone-ok",
    hint: "The agent may change path and query values (an id, an email) without asking. Method, URL, headers, body and auth stay as they are now",
    warning: `The agent can point this request at any record. ${INJECTION}`,
  },
  "auto-any": {
    access: "auto",
    scope: "any",
    label: "Auto: trusted, any edit",
    short: "Auto: trusted",
    icon: "lightning",
    tone: "tone-warn",
    hint: "The agent may edit anything in this request and send it without asking, on the same host and environment",
    warning: `The agent can rewrite the method, headers and body and send it with your auth. ${INJECTION}`,
  },
  blocked: { access: "blocked", scope: "exact", label: "Blocked", short: "Blocked", icon: "prohibit", tone: "tone-err", hint: "The agent cannot send this request" },
};

export const AGENT_MODE_ORDER: AgentMode[] = ["ask", "auto", "auto-params", "auto-any", "blocked"];

export function agentMode(access: AgentAccess, scope: GrantScope | undefined): AgentMode {
  if (access !== "auto") return access;
  return scope === "params" ? "auto-params" : scope === "any" ? "auto-any" : "auto";
}
