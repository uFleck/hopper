/* Pure logic of the environment editor: the editable draft of one application section, its validation and the missing-secrets checklist. */
import type { ApplicationEnvDto, ApplicationEnvInput, PatternRule, Stage, StageDto } from "../../ipc";

export const STAGES: Stage[] = ["local", "dev", "rc", "production"];

export interface DraftVariable {
  name: string;
  value: string;
  /** Comes from `*.local.yaml`; shown, not editable here. */
  local: boolean;
}

export interface EnvDraft {
  /** A section that does not exist yet in the stage. */
  isNew: boolean;
  stage: Stage;
  application: string;
  variables: DraftVariable[];
  secrets: { name: string; provider: string }[];
  insecureSkipVerify: boolean;
  hostsExplicit: boolean;
  hosts: string[];
  expectedRevision?: string;
}

export const blankDraft = (stage: Stage, application = ""): EnvDraft => ({
  isNew: true,
  stage,
  application,
  variables: [{ name: "baseUrl", value: "", local: false }],
  secrets: [],
  insecureSkipVerify: false,
  hostsExplicit: false,
  hosts: [],
});

export const draftFromApp = (stage: StageDto, a: ApplicationEnvDto): EnvDraft => ({
  isNew: false,
  stage: stage.stage,
  application: a.application,
  variables: a.variables.map((v) => ({ ...v })),
  secrets: a.secrets.map((s) => ({ name: s.name, provider: s.provider })),
  insecureSkipVerify: a.tls.insecureSkipVerify,
  hostsExplicit: a.allowedHostsExplicit,
  hosts: [...a.allowedHosts],
  expectedRevision: stage.revision ?? undefined,
});

export function draftToInput(d: EnvDraft): ApplicationEnvInput {
  return {
    stage: d.stage,
    application: d.application.trim(),
    variables: d.variables.filter((v) => v.name.trim()).map((v) => ({ name: v.name.trim(), value: v.value })),
    secrets: d.secrets.filter((s) => s.name.trim()).map((s) => ({ name: s.name.trim(), provider: s.provider })),
    tls: { insecureSkipVerify: d.insecureSkipVerify },
    allowedHosts: d.hostsExplicit ? d.hosts.map((h) => h.trim()).filter(Boolean) : undefined,
    expectedRevision: d.expectedRevision,
  };
}

export const isDirty = (a: EnvDraft, b: EnvDraft) => JSON.stringify(a) !== JSON.stringify(b);

// ------------------------------------------------------------------------------------------ validation

export interface DraftErrors {
  application?: string;
  variables: Record<number, string>;
  secrets: Record<number, string>;
  hosts: Record<number, string>;
}

const BAD_NAME_CHARS = /[\\/:*?"<>|]/;
const IDENT = /^[A-Za-z_][A-Za-z0-9_.-]*$/;

export function validateHost(h: string): string | undefined {
  const host = h.trim();
  if (!host) return "Enter a host, e.g. api.example.com";
  if (host === "*") return "A bare * would allow every host. Name the hosts you trust.";
  if (/\s/.test(host) || host.includes("://") || host.includes("/")) return "Host only: no scheme, path or spaces (api.example.com or api.example.com:8443).";
  return undefined;
}

export function validateDraft(d: EnvDraft, otherApps: string[]): DraftErrors {
  const errors: DraftErrors = { variables: {}, secrets: {}, hosts: {} };
  const name = d.application.trim();
  if (!name) errors.application = "Name the application.";
  else if (BAD_NAME_CHARS.test(name) || name.startsWith(".")) errors.application = 'The name becomes a file name: avoid \\ / : * ? " < > | and a leading dot.';
  else if (d.isNew && otherApps.some((n) => n.toLowerCase() === name.toLowerCase())) errors.application = "This stage already has a section for this application.";

  const seen = new Set<string>();
  const check = (list: { name: string }[], into: Record<number, string>) =>
    list.forEach((item, i) => {
      const n = item.name.trim();
      if (!n) return void (into[i] = "Name required.");
      if (!IDENT.test(n)) return void (into[i] = "Letters, digits, _ . - only; start with a letter or _.");
      if (seen.has(n)) return void (into[i] = "Already used by another variable or secret.");
      seen.add(n);
    });
  check(d.variables, errors.variables);
  check(d.secrets, errors.secrets);
  if (d.hostsExplicit) d.hosts.forEach((h, i) => void (validateHost(h) && (errors.hosts[i] = validateHost(h)!)));
  return errors;
}

export const hasErrors = (e: DraftErrors) =>
  !!e.application || Object.keys(e.variables).length > 0 || Object.keys(e.secrets).length > 0 || Object.keys(e.hosts).length > 0;

// ------------------------------------------------------------------------------------------ secrets

export interface MissingSecret {
  stage: Stage;
  application: string;
  name: string;
}

export function missingSecrets(stages: StageDto[]): MissingSecret[] {
  return stages.flatMap((st) =>
    st.applications.flatMap((a) => a.secrets.filter((s) => s.status === "missing").map((s) => ({ stage: st.stage, application: a.application, name: s.name }))),
  );
}

export const secretsToFillIn = (n: number) => `${n} secret${n === 1 ? "" : "s"} to fill in`;

// ------------------------------------------------------------------------------------- agent rules

/** Rules that would amount to "allow everything" are refused for production: only per-request Auto may open it up. */
export function validateRule(rule: PatternRule, stage: Stage): string | undefined {
  const host = rule.host.trim();
  const path = rule.path.trim();
  if (!host) return "Host required.";
  if (/\s/.test(host) || host.includes("://") || host.includes("/")) return "Host only, no scheme or path.";
  if (!path.startsWith("/")) return "The path glob starts with /, e.g. /api/users/*";
  if (stage === "production" && (host === "*" || /^\/(\*+)?$/.test(path) || path === "/**/*"))
    return "Too broad for production. Production is opened up one request at a time with Auto.";
  return undefined;
}

export function describeRule(rule: PatternRule): string {
  const verb = rule.method ? `${rule.method} requests` : "Any method";
  return `${verb} to ${rule.host || "(host)"} where the path matches ${rule.path || "(path)"}`;
}

export const sameRules = (a: { deny: boolean; safeReads: boolean; patterns: PatternRule[] }, b: typeof a) => JSON.stringify(a) === JSON.stringify(b);
