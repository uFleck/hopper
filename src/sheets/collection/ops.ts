/* Writes behind the collection Variables and Auth tabs. Each one rejects with a HopperError; callers turn that into a toast. */
import type { Stage, StageDto } from "../../ipc";
import { loadVariables, refreshEnvironments, saveVariables, setVariable, store } from "../../store";
import { clearSecretValue, saveEnvironment, setSecretValue } from "../env/api";
import { STAGES, blankDraft, draftFromApp, draftToInput, type EnvDraft } from "../env/draft";

const stageOf = (stage: Stage): StageDto | undefined => store.getState().environments.find((e) => e.stage === stage);

/** The draft of the collection's section in a stage as saved now; an empty one when the stage has no section yet. */
function sectionDraft(stage: StageDto, slug: string): EnvDraft {
  const a = stage.applications.find((x) => x.application === slug);
  return a ? draftFromApp(stage, a) : { ...blankDraft(stage.stage, slug), variables: [] };
}

async function writeSection(stage: Stage, slug: string, change: (d: EnvDraft) => EnvDraft | null) {
  const st = stageOf(stage);
  if (!st || st.readOnly) return;
  const next = change(sectionDraft(st, slug));
  if (!next) return;
  await saveEnvironment(draftToInput(next));
  await refreshEnvironments();
}

const collectionTarget = (slug: string) => ({ type: "collection" as const, slug });

export const setDefault = (slug: string, name: string, value: string) => setVariable(collectionTarget(slug), name, value);

/** Gives `name` its own value in one stage, over the collection default. */
export const setStageValue = (stage: Stage, slug: string, name: string, value: string) =>
  writeSection(stage, slug, (d) => ({ ...d, variables: [...d.variables.filter((v) => v.name !== name), { name, value, local: d.variables.find((v) => v.name === name)?.local ?? false }] }));

/** Drops a stage's own value so the collection default applies again. */
export const clearStageValue = (stage: Stage, slug: string, name: string) => writeSection(stage, slug, (d) => (d.variables.some((v) => v.name === name) ? { ...d, variables: d.variables.filter((v) => v.name !== name) } : null));

/** Declares the secret in the stage when it is not there yet, then stores the value on this machine. */
export async function setSecret(stage: Stage, slug: string, name: string, value: string) {
  await writeSection(stage, slug, (d) => (d.secrets.some((s) => s.name === name) ? null : { ...d, secrets: [...d.secrets, { name, provider: "keyring" }] }));
  await setSecretValue({ stage, application: slug }, name, value);
  await refreshEnvironments();
}

/** Declares a secret in every writable stage (so each shows its own "not set") and stores the values given. */
export async function addSecret(slug: string, name: string, values: Partial<Record<Stage, string>>) {
  for (const stage of STAGES) {
    const value = values[stage];
    if (value) await setSecret(stage, slug, name, value);
    else await writeSection(stage, slug, (d) => (d.secrets.some((s) => s.name === name) ? null : { ...d, secrets: [...d.secrets, { name, provider: "keyring" }] }));
  }
}

/** Gives a plain variable the same value in every environment, or one value per stage (empty ones are skipped). */
export async function addVariable(slug: string, name: string, value: string | Partial<Record<Stage, string>>) {
  if (typeof value === "string") {
    await setDefault(slug, name, value);
    return;
  }
  for (const stage of STAGES) if (value[stage]) await setStageValue(stage, slug, name, value[stage]!);
}

export async function clearSecret(stage: Stage, slug: string, name: string) {
  await clearSecretValue({ stage, application: slug }, name);
  await refreshEnvironments();
}

/** Removes the variable or secret from the collection and from every stage; stored secret values are forgotten. */
export async function removeVariable(slug: string, name: string, secret: boolean) {
  if (secret) {
    for (const stage of STAGES) {
      const has = stageOf(stage)?.applications.find((a) => a.application === slug)?.secrets.find((s) => s.name === name);
      if (has?.status === "set") await clearSecretValue({ stage, application: slug }, name);
      await writeSection(stage, slug, (d) => (d.secrets.some((s) => s.name === name) ? { ...d, secrets: d.secrets.filter((s) => s.name !== name) } : null));
    }
    return;
  }
  const target = collectionTarget(slug);
  const current = await loadVariables(target);
  if (current && current.variables.some((v) => v.name === name)) await saveVariables(target, current.variables.filter((v) => v.name !== name), current.revision);
  for (const stage of STAGES) await clearStageValue(stage, slug, name);
}
