import { useEffect, useMemo, useState } from "react";
import type { NameValue } from "../ipc";
import { loadVariables, saveVariables, targetKey, toast, useStore } from "../store";
import { Button, Icon, IconButton, TextInput } from "../ui";

const NAME_RE = /^[\w.-]+$/;

function problem(rows: NameValue[], i: number): string | null {
  const name = rows[i].name.trim();
  if (!name) return "Give the variable a name.";
  if (!NAME_RE.test(name)) return "Use letters, digits, dot, dash or underscore.";
  if (rows.some((r, j) => j !== i && r.name.trim() === name)) return "Already defined here.";
  return null;
}

/** The variables every request in a collection sees. Folded by default; edits are saved with one button. */
export function CollectionVariables({ slug, name }: { slug: string; name: string }) {
  const target = useMemo(() => ({ type: "collection" as const, slug }), [slug]);
  const loaded = useStore((s) => s.scopeVars[targetKey(target)]);
  const [open, setOpen] = useState(false);
  const [rows, setRows] = useState<NameValue[] | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (open) void loadVariables(target);
  }, [open, target]);

  // Rows follow what is on disk until the user edits them.
  const saved = loaded?.variables;
  const draft = rows ?? saved ?? [];
  const dirty = rows !== null && JSON.stringify(rows) !== JSON.stringify(saved ?? []);
  const errors = draft.map((_, i) => problem(draft, i));
  const invalid = errors.some(Boolean);

  const edit = (next: NameValue[]) => setRows(next);
  const save = async () => {
    setBusy(true);
    const ok = await saveVariables(target, draft.map((r) => ({ name: r.name.trim(), value: r.value })), loaded?.revision ?? null);
    setBusy(false);
    if (ok) {
      setRows(null);
      toast(`Saved variables of ${name}`, "success");
    }
  };

  return (
    <div className="col-vars">
      <button type="button" className="col-vars-head" aria-expanded={open} onClick={() => setOpen(!open)}>
        <Icon name={open ? "caret-down" : "caret-right"} />
        <span>Variables</span>
        {saved && <span className="col-count">{saved.length}</span>}
      </button>
      {open && (
        <div className="col-vars-body">
          <p className="col-vars-hint">Every request in {name} sees these. A folder or request variable with the same name wins; so does one here over the environment.</p>
          {draft.length === 0 && <p className="col-empty">No variables.</p>}
          {draft.map((v, i) => (
            <div key={i}>
              <div className="sh-var-row">
                <TextInput mono aria-label="Variable name" placeholder="baseUrl" value={v.name} invalid={!!errors[i] && !!v.name} onChange={(e) => edit(draft.map((r, j) => (j === i ? { ...r, name: e.target.value } : r)))} />
                <TextInput mono aria-label={`Value of ${v.name || "variable"}`} placeholder="value" value={v.value} onChange={(e) => edit(draft.map((r, j) => (j === i ? { ...r, value: e.target.value } : r)))} />
                <IconButton icon="trash" label={`Remove ${v.name || "variable"}`} size="sm" tone="danger" onClick={() => edit(draft.filter((_, j) => j !== i))} />
              </div>
              {errors[i] && v.name && <p className="ui-field-error" role="alert">{errors[i]}</p>}
            </div>
          ))}
          <div className="col-block-actions">
            <Button size="sm" icon="plus" onClick={() => edit([...draft, { name: "", value: "" }])}>Add variable</Button>
            {dirty && (
              <>
                <Button size="sm" variant="primary" loading={busy} disabled={invalid} onClick={() => void save()}>Save</Button>
                <Button size="sm" variant="ghost" disabled={busy} onClick={() => setRows(null)}>Discard</Button>
              </>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
