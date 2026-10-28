import { toHopperError, type StageDto } from "../../ipc";
import { refreshEnvironments, setVariable, store, toast } from "../../store";
import { blankDraft, draftFromApp, draftToInput } from "../../sheets/env/draft";
import { saveEnvironment } from "../../sheets/env/api";
import type { VarEdit } from "./logic";

/** Sets `name` in the stage's section for `application`, on top of what is saved now. A value from `*.local.yaml` stays local. */
export async function setEnvVariable(stage: StageDto, application: string, name: string, value: string) {
  const section = stage.applications.find((a) => a.application === application);
  const base = section ? draftFromApp(stage, section) : { ...blankDraft(stage.stage, application), variables: [] };
  const local = base.variables.find((v) => v.name === name)?.local ?? false;
  const variables = [...base.variables.filter((v) => v.name !== name), { name, value, local }];
  await saveEnvironment(draftToInput({ ...base, variables }));
  await refreshEnvironments();
}

/** Writes a variable's new value where its winning value lives. Failures show as a toast; resolves to whether it saved. */
export async function saveVariableValue(edit: VarEdit, name: string, value: string): Promise<boolean> {
  if (edit.type === "scope") return setVariable(edit.target, name, value);
  const stage = store.getState().environments.find((e) => e.stage === edit.stage);
  if (!stage || stage.readOnly) return false;
  try {
    await setEnvVariable(stage, edit.application, name, value);
    return true;
  } catch (e) {
    toast(toHopperError(e).message, "error");
    return false;
  }
}
