import { useRef, useState } from "react";
import { toHopperError, type StageDto } from "../../ipc";
import { dismissAddVariable, setVariable, toast } from "../../store";
import { Button, Field, Select, TextInput, useFocusTrap } from "../../ui";
import { setEnvVariable } from "./varEdit";

type Scope = "collection" | "environment" | "workspace";

export interface AddVariableDialogProps {
  name: string;
  collection: string | null;
  /** The active stage, when there is one the user may edit. */
  stage: StageDto | null;
}

/** Defines a variable the request uses but nothing defines yet. Collection is the default; the active environment or the workspace are the alternatives. */
export function AddVariableDialog({ name, collection, stage }: AddVariableDialogProps) {
  const scopes: { value: Scope; label: string }[] = [
    ...(collection ? [{ value: "collection" as const, label: `Collection ${collection}` }] : []),
    ...(collection && stage && !stage.readOnly ? [{ value: "environment" as const, label: `${stage.stage} environment` }] : []),
    { value: "workspace", label: "Whole workspace" },
  ];
  const [scope, setScope] = useState<Scope>(scopes[0].value);
  const [value, setValue] = useState("");
  const [busy, setBusy] = useState(false);
  const ref = useRef<HTMLFormElement>(null);
  const valueRef = useRef<HTMLInputElement>(null);
  useFocusTrap(ref, true, valueRef);

  const submit = async () => {
    if (!value) return;
    setBusy(true);
    let ok = false;
    try {
      if (scope === "environment" && stage && collection) {
        await setEnvVariable(stage, collection, name, value);
        ok = true;
      } else {
        ok = await setVariable(scope === "collection" && collection ? { type: "collection", slug: collection } : { type: "workspace" }, name, value);
      }
    } catch (e) {
      toast(toHopperError(e).message, "error");
    }
    setBusy(false);
    if (!ok) return;
    toast(`Added {{${name}}} to ${scopes.find((s) => s.value === scope)?.label.toLowerCase()}`, "success");
    dismissAddVariable();
  };

  return (
    <div className="ui-scrim" onMouseDown={(e) => e.stopPropagation()}>
      <form
        ref={ref}
        className="ui-dialog rv-savedlg"
        role="dialog"
        aria-modal="true"
        aria-label="Add variable"
        onSubmit={(e) => (e.preventDefault(), void submit())}
        onKeyDown={(e) => e.key === "Escape" && (e.preventDefault(), e.stopPropagation(), dismissAddVariable())}
      >
        <h3 className="ui-dialog-title">Add variable</h3>
        <p className="ui-dialog-msg">
          <code>{`{{${name}}}`}</code> has no value yet.
        </p>
        <Field label="Value">{(id) => <TextInput id={id} ref={valueRef} mono value={value} placeholder="value" onChange={(e) => setValue(e.target.value)} />}</Field>
        <Field label="Save in" hint={scope === "environment" ? "Shared in git, like the rest of the environment." : scope === "collection" ? "Every request in the collection sees it." : undefined}>
          {(id) => (
            <Select id={id} value={scope} onChange={(e) => setScope(e.target.value as Scope)}>
              {scopes.map((s) => (
                <option key={s.value} value={s.value}>{s.label}</option>
              ))}
            </Select>
          )}
        </Field>
        <div className="ui-dialog-actions">
          <Button onClick={dismissAddVariable}>Cancel</Button>
          <Button type="submit" variant="primary" loading={busy} disabled={!value}>Add</Button>
        </div>
      </form>
    </div>
  );
}
