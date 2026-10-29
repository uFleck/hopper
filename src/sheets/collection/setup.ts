/* The setup checklist of a collection: what is still missing before its requests can run in the active environment. */
import type { Auth, Stage } from "../../ipc";
import type { CollectionTab } from "../../store";
import { authText, variablesIn } from "../../views/request/logic";
import { STAGE_LABEL, gapsIn, isRawValue, type MatrixRow } from "./matrix";

export interface SetupItem {
  key: "spec" | "base" | "auth" | "values";
  /** `ready` and `todo` count toward the progress; `optional` does not. */
  state: "ready" | "todo" | "optional";
  title: string;
  detail: string;
  go?: { label: string; tab: CollectionTab };
}

const AUTH_NAME: Record<string, string> = { bearer: "Bearer token", basic: "Basic", apiKey: "API key" };

export function setupItems(input: { operations: number; rows: MatrixRow[]; auth: Auth | undefined; active: Stage | null }): SetupItem[] {
  const { operations, rows, auth, active } = input;
  const where = active ? STAGE_LABEL[active] : null;
  const gaps = active ? gapsIn(rows, active) : [];
  const items: SetupItem[] = [];

  items.push(operations > 0
    ? { key: "spec", state: "ready", title: "Spec imported", detail: `${operations} operations` }
    : { key: "spec", state: "optional", title: "No spec imported", detail: "Optional: import one to get every endpoint", go: { label: "Import", tab: "specs" } });

  const base = rows.find((r) => r.name === "baseUrl");
  const baseOk = !!base && (!active || base.cells[active].kind !== "empty");
  items.push(baseOk
    ? { key: "base", state: "ready", title: "Base URL", detail: "baseUrl is set" }
    : { key: "base", state: "todo", title: "Base URL", detail: base && where ? `baseUrl has no value in ${where}` : "Add a baseUrl variable", go: { label: base ? "Set" : "Add", tab: "variables" } });

  if (!auth || auth.type === "none" || auth.type === "inherit") {
    items.push({ key: "auth", state: "optional", title: "Authentication", detail: "None. Set it if the API needs a login", go: { label: "Set up", tab: "auth" } });
  } else {
    const text = authText(auth);
    const raw = [auth.type === "bearer" ? auth.token : auth.type === "basic" ? auth.password : auth.value].some(isRawValue);
    const need = rows.filter((r) => r.secret && text.some((t) => variablesIn(t).includes(r.name)) && active && r.cells[active].kind !== "set");
    if (raw) items.push({ key: "auth", state: "todo", title: "Authentication", detail: "A credential is written in the collection file", go: { label: "Fix", tab: "auth" } });
    else if (need.length > 0 && where) items.push({ key: "auth", state: "todo", title: "Authentication", detail: `${AUTH_NAME[auth.type]} needs {{${need[0].name}}} in ${where}`, go: { label: "Set value", tab: "auth" } });
    else items.push({ key: "auth", state: "ready", title: "Authentication", detail: `${AUTH_NAME[auth.type]} ready` });
  }

  if (where) {
    items.push(gaps.length > 0
      ? { key: "values", state: "todo", title: "Values", detail: `${gaps.length} variable${gaps.length === 1 ? "" : "s"} without a value in ${where}`, go: { label: "Review", tab: "variables" } }
      : { key: "values", state: "ready", title: "Values", detail: `Everything has a value in ${where}` });
  }
  return items;
}

export const progress = (items: SetupItem[]) => {
  const counted = items.filter((i) => i.state !== "optional");
  return { done: counted.filter((i) => i.state === "ready").length, total: counted.length };
};
