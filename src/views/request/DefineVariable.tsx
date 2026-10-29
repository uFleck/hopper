import { useRef, useState } from "react";
import { toHopperError, type StageDto } from "../../ipc";
import { openSheet, setVariable, toast, useStore } from "../../store";
import { Button, cx } from "../../ui";
import { setEnvVariable } from "./varEdit";

export type DefineScope = "collection" | "environment" | "workspace";

const LOOKS_SECRET = /token|secret|passw|key$|bearer|credential|private/i;

export function defineScopes(collection: string | null, stage: StageDto | null): { value: DefineScope; label: string; hint: string }[] {
  return [
    ...(collection ? [{ value: "collection" as const, label: "Collection", hint: `Every request in ${collection} sees it.` }] : []),
    ...(collection && stage && !stage.readOnly ? [{ value: "environment" as const, label: `${stage.stage} env`, hint: `Only in the ${stage.stage} environment. Shared in git, like the rest of it.` }] : []),
    { value: "workspace", label: "Workspace", hint: "Every request in every collection sees it." },
  ];
}

export interface DefineVariableProps {
  name: string;
  collection: string | null;
  /** Called after the variable is saved. */
  onDone: () => void;
  autoFocus?: boolean;
}

/** Gives a variable that nothing defines its first value, in the scope the user picks. Shared by the hover card and the add dialog. */
export function DefineVariable({ name, collection, onDone, autoFocus }: DefineVariableProps) {
  const stage = useStore((s) => s.environments.find((e) => e.active) ?? null);
  const scopes = defineScopes(collection, stage);
  const [scope, setScope] = useState<DefineScope>(scopes[0].value);
  const [value, setValue] = useState("");
  const [busy, setBusy] = useState(false);
  const input = useRef<HTMLInputElement>(null);
  const current = scopes.find((s) => s.value === scope) ?? scopes[0];

  const submit = async () => {
    if (!value) return;
    setBusy(true);
    let ok = false;
    try {
      if (current.value === "environment" && stage && collection) {
        await setEnvVariable(stage, collection, name, value);
        ok = true;
      } else {
        ok = await setVariable(current.value === "collection" && collection ? { type: "collection", slug: collection } : { type: "workspace" }, name, value);
      }
    } catch (e) {
      toast(toHopperError(e).message, "error");
    }
    setBusy(false);
    if (!ok) return;
    toast(`Added {{${name}}} to ${current.value === "environment" ? `${current.label} environment` : current.label.toLowerCase()}`, "success");
    onDone();
  };

  return (
    <form className="vp-form" onSubmit={(e) => (e.preventDefault(), void submit())}>
      <div className="vp-edit">
        <input
          ref={input}
          className="ui-input ui-input--mono"
          aria-label={`Value of ${name}`}
          value={value}
          placeholder="value"
          spellCheck={false}
          autoFocus={autoFocus}
          onChange={(e) => setValue(e.target.value)}
        />
        <Button type="submit" variant="primary" size="sm" disabled={!value} loading={busy}>Add</Button>
      </div>
      {scopes.length > 1 && (
        <div className="vp-scopes" role="radiogroup" aria-label="Save in">
          {scopes.map((s) => (
            <button key={s.value} type="button" role="radio" aria-checked={s.value === scope} className={cx("vp-scope", s.value === scope && "is-on")} onClick={() => (setScope(s.value), input.current?.focus())}>
              {s.label}
            </button>
          ))}
        </div>
      )}
      <p className="vp-foot">{current.hint}</p>
      {LOOKS_SECRET.test(name) && (
        <p className="vp-foot vp-foot--warn">
          Looks like a credential. <button type="button" className="vp-link" onClick={() => openSheet("environments")}>Store it as a secret</button> to keep it out of git.
        </p>
      )}
    </form>
  );
}
