import { useRef } from "react";
import { dismissAddVariable } from "../../store";
import { Button, useFocusTrap } from "../../ui";
import { DefineVariable } from "./DefineVariable";

export interface AddVariableDialogProps {
  name: string;
  collection: string | null;
}

/** Defines a variable the request uses but nothing defines yet, from the chips in the URL bar. Collection is the default scope. */
export function AddVariableDialog({ name, collection }: AddVariableDialogProps) {
  const ref = useRef<HTMLDivElement>(null);
  useFocusTrap(ref, true);

  return (
    <div className="ui-scrim" onMouseDown={(e) => e.stopPropagation()}>
      <div
        ref={ref}
        className="ui-dialog rv-savedlg"
        role="dialog"
        aria-modal="true"
        aria-label="Add variable"
        onKeyDown={(e) => e.key === "Escape" && (e.preventDefault(), e.stopPropagation(), dismissAddVariable())}
      >
        <h3 className="ui-dialog-title">Add variable</h3>
        <p className="ui-dialog-msg">
          <code>{`{{${name}}}`}</code> has no value yet.
        </p>
        <DefineVariable name={name} collection={collection} autoFocus onDone={dismissAddVariable} />
        <div className="ui-dialog-actions">
          <Button onClick={dismissAddVariable}>Cancel</Button>
        </div>
      </div>
    </div>
  );
}
