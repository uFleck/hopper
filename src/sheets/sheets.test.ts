import { describe, expect, it } from "vitest";
import type { ApplicationEnvDto, StageDto } from "../ipc";
import { blankDraft, draftFromApp, draftToInput, hasErrors, missingSecrets, secretsToFillIn, validateDraft, validateHost, validateRule } from "./env/draft";
import { comboFromEvent } from "./hotkey";

const app = (over: Partial<ApplicationEnvDto> = {}): ApplicationEnvDto => ({
  application: "account",
  variables: [{ name: "baseUrl", value: "https://s.example.com", local: false }, { name: "tenant", value: "me", local: true }],
  secrets: [{ name: "apiKey", provider: "keyring", status: "missing" }, { name: "pw", provider: "keyring", status: "set" }],
  tls: { insecureSkipVerify: false }, allowedHosts: ["s.example.com"], allowedHostsExplicit: false, ...over,
});

const stage = (applications: ApplicationEnvDto[] = [app()], over: Partial<StageDto> = {}): StageDto => ({
  stage: "rc", active: false, applications, revision: "r1", readOnly: false,
  agentRules: { deny: false, safeReads: true, patterns: [] }, ...over,
});

describe("environment draft", () => {
  it("round-trips and omits derived hosts", () => {
    const input = draftToInput(draftFromApp(stage(), app()));
    expect(input.allowedHosts).toBeUndefined();
    expect(input.expectedRevision).toBe("r1");
    expect(input.secrets).toEqual([{ name: "apiKey", provider: "keyring" }, { name: "pw", provider: "keyring" }]);
  });

  it("sends explicit hosts and no revision for a new environment", () => {
    const d = { ...blankDraft("dev"), application: " x ", hostsExplicit: true, hosts: ["a.com", " "] };
    const input = draftToInput(d);
    expect(input.application).toBe("x");
    expect(input.stage).toBe("dev");
    expect(input.allowedHosts).toEqual(["a.com"]);
    expect(input.expectedRevision).toBeUndefined();
  });

  it("validates names, duplicates and hosts", () => {
    const d = { ...draftFromApp(stage(), app()), isNew: true, application: "account", hostsExplicit: true, hosts: ["https://x.com"] };
    d.variables = [...d.variables, { name: "apiKey", value: "", local: false }];
    const e = validateDraft(d, ["account"]);
    expect(e.application).toMatch(/already has/);
    expect(e.secrets[0]).toMatch(/Already used/);
    expect(e.hosts[0]).toMatch(/no scheme/);
    expect(hasErrors(validateDraft({ ...d, application: "ok", variables: [], hostsExplicit: false }, []))).toBe(false);
  });

  it("refuses a bare wildcard host", () => {
    expect(validateHost("*")).toBeTruthy();
    expect(validateHost("*.example.com")).toBeUndefined();
  });
});

describe("secrets checklist", () => {
  it("lists missing secrets across environments", () => {
    expect(missingSecrets([stage(), stage([app({ application: "billing" })], { stage: "dev" })])).toEqual([{ stage: "rc", application: "account", name: "apiKey" }, { stage: "dev", application: "billing", name: "apiKey" }]);
    expect(secretsToFillIn(1)).toBe("1 secret to fill in");
    expect(secretsToFillIn(3)).toBe("3 secrets to fill in");
  });
});

describe("agent pattern rules", () => {
  it("accepts a narrow rule and refuses blanket ones on production", () => {
    expect(validateRule({ method: "GET", host: "api.x.com", path: "/users/*" }, "production")).toBeUndefined();
    expect(validateRule({ host: "api.x.com", path: "/**" }, "production")).toMatch(/production/);
    expect(validateRule({ host: "*", path: "/users" }, "production")).toMatch(/production/);
    expect(validateRule({ host: "api.x.com", path: "/**" }, "dev")).toBeUndefined();
    expect(validateRule({ host: "api.x.com", path: "users" }, "dev")).toMatch(/starts with/);
  });
});

describe("hotkey recording", () => {
  const key = (code: string, mods: Partial<Record<"ctrlKey" | "altKey" | "shiftKey" | "metaKey", boolean>> = {}) =>
    comboFromEvent({ code, ctrlKey: false, altKey: false, shiftKey: false, metaKey: false, ...mods });
  it("builds a combo in a stable order", () => {
    expect(key("KeyH", { altKey: true, ctrlKey: true })).toEqual({ ok: true, combo: "Ctrl+Alt+H" });
    expect(key("F5", { shiftKey: true, metaKey: true })).toEqual({ ok: true, combo: "Shift+Super+F5" });
    expect(key("ArrowUp", { ctrlKey: true })).toEqual({ ok: true, combo: "Ctrl+Up" });
  });
  it("rejects modifier-only, unmodified and unsupported keys", () => {
    expect(key("ControlLeft", { ctrlKey: true })).toEqual({ ok: false, reason: "modifier-only" });
    expect(key("KeyH")).toEqual({ ok: false, reason: "needs-modifier" });
    expect(key("KeyH", { shiftKey: true })).toEqual({ ok: false, reason: "needs-modifier" });
    expect(key("NumLock", { ctrlKey: true })).toEqual({ ok: false, reason: "unsupported" });
  });
});
