/* Pure logic of the collection Variables tab: every variable of one collection, with its default and its value (or secret status) in each stage. */
import type { NameValue, Stage, StageDto } from "../../ipc";
import { STAGES } from "../env/draft";

export { STAGES };

export const STAGE_LABEL: Record<Stage, string> = { local: "local", dev: "dev", rc: "rc", production: "prod" };

export type Cell =
  /** A value written for this stage; `local` means it comes from `*.local.yaml`. */
  | { kind: "value"; value: string; local: boolean }
  /** No value for this stage: the collection default applies. */
  | { kind: "inherit"; value: string }
  /** Nothing defines it in this stage and there is no default. */
  | { kind: "empty" }
  | { kind: "set" }
  | { kind: "missing" }
  /** A secret declared in another stage but not in this one. */
  | { kind: "undeclared" };

export interface MatrixRow {
  name: string;
  secret: boolean;
  /** The collection-level value; null when only stages define it. Always null for secrets. */
  defaultValue: string | null;
  cells: Record<Stage, Cell>;
}

const sectionOf = (stage: StageDto | undefined, slug: string) => stage?.applications.find((a) => a.application === slug);

export function buildMatrix(defaults: NameValue[], stages: StageDto[], slug: string): MatrixRow[] {
  const byStage = (s: Stage) => stages.find((x) => x.stage === s);
  const secretNames = new Set(STAGES.flatMap((s) => sectionOf(byStage(s), slug)?.secrets.map((x) => x.name) ?? []));
  const names: string[] = [];
  const add = (n: string) => {
    if (!secretNames.has(n) && !names.includes(n)) names.push(n);
  };
  defaults.forEach((d) => add(d.name));
  STAGES.forEach((s) => sectionOf(byStage(s), slug)?.variables.forEach((v) => add(v.name)));

  const variables = names.map((name): MatrixRow => {
    const def = defaults.find((d) => d.name === name)?.value ?? null;
    const cells = {} as Record<Stage, Cell>;
    for (const s of STAGES) {
      const v = sectionOf(byStage(s), slug)?.variables.find((x) => x.name === name);
      cells[s] = v ? { kind: "value", value: v.value, local: v.local } : def !== null ? { kind: "inherit", value: def } : { kind: "empty" };
    }
    return { name, secret: false, defaultValue: def, cells };
  });

  const secrets = [...secretNames].map((name): MatrixRow => {
    const cells = {} as Record<Stage, Cell>;
    for (const s of STAGES) {
      const found = sectionOf(byStage(s), slug)?.secrets.find((x) => x.name === name);
      cells[s] = found ? { kind: found.status } : { kind: "undeclared" };
    }
    return { name, secret: true, defaultValue: null, cells };
  });

  return [...variables, ...secrets];
}

/** A cell that still needs the user: a secret without a value, or a variable nothing defines in the stage. */
export const needsValue = (c: Cell) => c.kind === "missing" || c.kind === "undeclared" || c.kind === "empty";

export const gapsIn = (rows: MatrixRow[], stage: Stage) => rows.filter((r) => needsValue(r.cells[stage]));

export interface SecretGap {
  application: string;
  name: string;
  /** False when the stage has no entry for the secret yet (declared only in another stage). */
  declared: boolean;
}

/** Secrets of every collection that have no value in `stage`. Computed from the environments alone, so it works for collections nobody opened. */
export function secretGaps(stages: StageDto[], stage: Stage): SecretGap[] {
  const apps = new Set(stages.flatMap((s) => s.applications.map((a) => a.application)));
  const here = stages.find((s) => s.stage === stage);
  const out: SecretGap[] = [];
  for (const application of [...apps].sort((a, b) => a.localeCompare(b))) {
    const names = new Set(stages.flatMap((s) => sectionOf(s, application)?.secrets.map((x) => x.name) ?? []));
    for (const name of names) {
      const found = sectionOf(here, application)?.secrets.find((x) => x.name === name);
      if (!found) out.push({ application, name, declared: false });
      else if (found.status === "missing") out.push({ application, name, declared: true });
    }
  }
  return out;
}

export function groupByApplication(gaps: SecretGap[]): { application: string; gaps: SecretGap[] }[] {
  const map = new Map<string, SecretGap[]>();
  for (const g of gaps) map.set(g.application, [...(map.get(g.application) ?? []), g]);
  return [...map].map(([application, list]) => ({ application, gaps: list }));
}

export const VARIABLE_NAME = /^[A-Za-z_][\w.-]*$/;

/** Why `name` cannot be used for a new variable of the collection, or null. */
export function nameProblem(name: string, rows: MatrixRow[]): string | null {
  const n = name.trim();
  if (!n) return null;
  if (!VARIABLE_NAME.test(n)) return "Letters, digits, _ . - only; start with a letter or _.";
  if (rows.some((r) => r.name === n)) return "Already defined in this collection. Edit it in the list instead.";
  return null;
}

/** Credential-looking names are made secrets by default. */
export const LOOKS_SECRET = /token|secret|passw|key$|bearer|credential|private/i;

/** A value typed into an auth field that is not a `{{variable}}`: it would be written into the shared collection file. */
export const isRawValue = (value: string) => value.trim() !== "" && !/\{\{[^}]*\}\}/.test(value);
