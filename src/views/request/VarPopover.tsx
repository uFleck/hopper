import { useState } from "react";
import { openSheet, toast } from "../../store";
import { Button, Icon, cx } from "../../ui";
import { DefineVariable } from "./DefineVariable";
import { closestNames, isBuiltin, type VarInfo, type VarScopeKind } from "./logic";
import { saveVariableValue } from "./varEdit";

const KIND_LABEL: Record<VarScopeKind, string> = {
  request: "Request",
  folder: "Folder",
  collection: "Collection",
  environment: "Environment",
  workspace: "Workspace",
};

/**
 * What the card shows for `{{name}}`: its type, where it lives, and its value to edit in place.
 * A name nothing defines gets the fix right in the card: names it may be a typo of (`rename` rewrites the text) and a value to give it.
 */
export function VarPopover({ name, info, rename, canDefine = true }: { name: string; info: VarInfo; rename?: (name: string) => void; canDefine?: boolean }) {
  const source = info.sources.get(name);
  const secret = info.secrets.get(name);
  const saved = info.values.get(name) ?? "";
  const [draft, setDraft] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const value = draft ?? saved;
  const dirty = draft !== null && draft !== saved;
  const edit = source?.edit ?? null;

  const save = async () => {
    if (!edit || !dirty) return;
    setBusy(true);
    const ok = await saveVariableValue(edit, name, value);
    setBusy(false);
    if (ok) {
      setDraft(null);
      toast(`Saved ${name}`, "success");
    }
  };

  const builtin = isBuiltin(name);
  const similar = !source && !secret && !builtin ? closestNames(name, info) : [];
  const kind = builtin ? "builtin" : secret ? "secret" : (source?.kind ?? "none");
  const kindLabel = builtin ? "Built-in" : secret ? "Secret" : source ? KIND_LABEL[source.kind] : "Not defined";

  return (
    <div className="vp" role="group" aria-label={`Variable ${name}`}>
      <div className="vp-head">
        <code className="vp-name">{name}</code>
        <span className={cx("vp-kind", `vp-kind--${kind}`)}>{kindLabel}</span>
      </div>
      {source?.detail && !secret && <p className="vp-where"><Icon name="map-pin-simple" />{source.detail}</p>}

      {builtin && <p className="vp-note">Generated again each time the request is sent.</p>}
      {secret && (
        <p className="vp-note vp-note--icon">
          <Icon name="lock-simple" />
          {secret === "set" ? "Stored on this machine. Its value never shows here." : "No value stored yet, so requests send it empty."}
        </p>
      )}

      {source && !secret && (
        <form className="vp-form" onSubmit={(e) => (e.preventDefault(), void save())}>
          <label className="vp-label" htmlFor={`vp-${name}`}>Value</label>
          <div className="vp-edit">
            <input
              id={`vp-${name}`}
              className="ui-input ui-input--mono"
              value={value}
              disabled={!edit || busy}
              spellCheck={false}
              placeholder="empty"
              onChange={(e) => setDraft(e.target.value)}
              onKeyDown={(e) => e.key === "Escape" && draft !== null && (e.stopPropagation(), setDraft(null))}
            />
            {edit && <Button type="submit" variant="primary" disabled={!dirty} loading={busy}>Save</Button>}
          </div>
          <p className="vp-foot">{!edit ? "This environment is read-only." : dirty ? "Enter to save · Esc to revert" : "Edit the value here; it saves to its scope."}</p>
        </form>
      )}

      {secret === "missing" && <Button icon="lock-simple" className="vp-add" onClick={() => openSheet("environments")}>Set its value in Environments</Button>}

      {!source && !secret && !builtin && (
        <>
          {similar.length > 0 && rename && (
            <div className="vp-similar">
              <span className="vp-label">Did you mean</span>
              <div className="vp-chips">
                {similar.map((n) => (
                  <button key={n} type="button" className="vp-chip" onClick={() => rename(n)}>
                    {n}
                    <span>{info.secrets.has(n) ? "secret" : (info.sources.get(n)?.kind ?? "")}</span>
                  </button>
                ))}
              </div>
            </div>
          )}
          {canDefine ? (
            <>
              <span className="vp-label">{similar.length > 0 ? "Or define it" : "Nothing defines it yet. Give it a value"}</span>
              <DefineVariable name={name} collection={info.collection} autoFocus={similar.length === 0} onDone={() => undefined} />
            </>
          ) : (
            <p className="vp-note">Nothing defines it yet. Add a row for it in this list.</p>
          )}
        </>
      )}
    </div>
  );
}
