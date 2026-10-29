import { useEffect, useMemo } from "react";
import type { NameValue } from "../../ipc";
import { loadVariables, scopeLayers, scopeTargets, useStore } from "../../store";
import { resolveEnv, varInfo, type VarInfo } from "./logic";

/**
 * What `{{name}}` resolves to for a collection's own settings (its variables and its auth): the workspace, the collection and the active environment.
 * `draft` stands in for the collection's saved variables, so a name typed a moment ago already counts.
 */
export function useCollectionInfo(slug: string, enabled: boolean, draft?: NameValue[]): VarInfo {
  const stage = useStore((s) => s.environments.find((e) => e.active));
  const scopeVars = useStore((s) => s.scopeVars);
  const targets = useMemo(() => scopeTargets(slug, [], null), [slug]);

  useEffect(() => {
    if (enabled) targets.forEach((t) => void loadVariables(t.target));
  }, [enabled, targets]);

  return useMemo(() => {
    const layers = scopeLayers(scopeVars, targets);
    if (draft) {
      const vars = draft.filter((v) => v.name.trim()).map((v) => ({ name: v.name.trim(), value: v.value }));
      const at = layers.above.findIndex((l) => l.target.type === "collection");
      if (at >= 0) layers.above[at] = { ...layers.above[at], vars };
      else layers.above.unshift({ origin: `collection ${slug}`, vars, target: { type: "collection", slug } });
    }
    return varInfo(resolveEnv(stage, slug), layers, slug);
  }, [stage, slug, scopeVars, targets, draft]);
}
