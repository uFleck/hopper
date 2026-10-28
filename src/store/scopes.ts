/* Variable scopes beside the environment: which ones a request resolves against, and how loaded ones layer. Pure, no store access. */
import type { NameValue, VarTargetDto, VariablesDto } from "../ipc";

/** One scope's variables with the label shown as their origin, e.g. "collection account-api". */
export interface VarLayer {
  origin: string;
  vars: NameValue[];
  target: VarTargetDto;
}

/** The scopes a request resolves against besides the environment, lowest precedence first. */
export interface ScopeLayers {
  /** Under the environment: workspace defaults. */
  below: VarLayer[];
  /** Over the environment: collection, folders (outermost first), request. */
  above: VarLayer[];
}

export const NO_LAYERS: ScopeLayers = { below: [], above: [] };

export const targetKey = (t: VarTargetDto) => JSON.stringify(t);

export interface ScopeTarget {
  target: VarTargetDto;
  origin: string;
}

/** The variable scopes of a request, lowest precedence first: workspace, collection, each folder from the root, the request itself. */
export function scopeTargets(collection: string | null | undefined, folder: string[], requestId: string | null): ScopeTarget[] {
  const out: ScopeTarget[] = [{ target: { type: "workspace" }, origin: "workspace" }];
  if (collection) {
    out.push({ target: { type: "collection", slug: collection }, origin: `collection ${collection}` });
    folder.forEach((_, i) => {
      const path = folder.slice(0, i + 1);
      out.push({ target: { type: "folder", collection, path }, origin: `folder ${path.join("/")}` });
    });
  }
  if (requestId) out.push({ target: { type: "request", id: requestId }, origin: "this request" });
  return out;
}

/** Splits the loaded scopes around the environment: workspace defaults below it, everything else above. */
export function scopeLayers(loaded: Record<string, VariablesDto>, targets: ScopeTarget[]): ScopeLayers {
  const layers: ScopeLayers = { below: [], above: [] };
  for (const { target, origin } of targets) {
    const dto = loaded[targetKey(target)];
    if (dto) (target.type === "workspace" ? layers.below : layers.above).push({ origin, vars: dto.variables, target });
  }
  return layers;
}

/** `vars` with `name` set to `value`: replaced in place when present, appended otherwise. */
export function withVariable(vars: NameValue[], name: string, value: string): NameValue[] {
  return vars.some((v) => v.name === name) ? vars.map((v) => (v.name === name ? { name, value } : v)) : [...vars, { name, value }];
}
