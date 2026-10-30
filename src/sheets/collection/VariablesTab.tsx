import { useEffect, useMemo, useState } from "react";
import type { Stage } from "../../ipc";
import { loadVariables, openEnvSettings, targetKey, useStore } from "../../store";
import { Button, Icon, IconButton, confirm, cx } from "../../ui";
import { useCollectionInfo } from "../../views/request/useCollectionInfo";
import { VarInput } from "../../views/request/VarInput";
import { AddVariableForm } from "./AddVariableForm";
import { SecretValueForm, attempt } from "./controls";
import { STAGES, STAGE_LABEL, buildMatrix, needsValue, type Cell, type MatrixRow } from "./matrix";
import { clearSecret, clearStageValue, removeVariable, setDefault, setSecret, setStageValue } from "./ops";

type Column = "default" | Stage;

function cellText(c: Cell): { text: string; tone: string; title?: string } {
  switch (c.kind) {
    case "value": return { text: c.value || "empty", tone: "is-own", title: c.local ? `${c.value} (from your local file)` : c.value };
    case "inherit": return { text: "default", tone: "is-inherit", title: `Uses the default: ${c.value}` };
    case "set": return { text: "set", tone: "is-set", title: "A value is stored on this machine" };
    case "missing": return { text: "not set", tone: "is-gap", title: "No value stored yet" };
    case "undeclared": return { text: "not set", tone: "is-gap", title: "Not declared in this environment yet" };
    default: return { text: "none", tone: "is-gap", title: "Nothing defines it here" };
  }
}

/** Every variable of the collection in one grid: its default, then its value or secret status in each environment. Click a cell to edit it. */
export function VariablesTab({ slug, name }: { slug: string; name: string }) {
  const target = useMemo(() => ({ type: "collection" as const, slug }), [slug]);
  const loaded = useStore((s) => s.scopeVars[targetKey(target)]);
  const stages = useStore((s) => s.environments);
  const active = stages.find((s) => s.active)?.stage ?? null;
  const [adding, setAdding] = useState(false);
  const [edit, setEdit] = useState<{ name: string; column: Column } | null>(null);

  useEffect(() => void loadVariables(target), [target]);
  const rows = useMemo(() => buildMatrix(loaded?.variables ?? [], stages, slug), [loaded, stages, slug]);
  const readOnly = (s: Stage) => !!stages.find((x) => x.stage === s)?.readOnly;

  const remove = async (row: MatrixRow) => {
    const ok = await confirm({
      title: `Delete ${row.name}?`,
      message: row.secret ? `${row.name} is removed from every environment of ${name} and its stored values are forgotten on this machine.` : `${row.name} is removed from ${name} and from every environment that overrides it. Requests that use it will flag it as undefined.`,
      confirmLabel: "Delete",
      tone: "danger",
    });
    if (ok) await attempt(() => removeVariable(slug, row.name, row.secret), `Deleted ${row.name}`);
  };

  return (
    <div className="cv">
      <div className="cv-bar">
        <p className="cv-muted">Every request in {name} sees these. Click a value to change it; a dashed cell uses the default.</p>
        {!adding && <Button size="sm" icon="plus" onClick={() => setAdding(true)}>Variable</Button>}
      </div>

      {adding && <AddVariableForm slug={slug} rows={rows} onCancel={() => setAdding(false)} onDone={() => setAdding(false)} />}

      {rows.length === 0 && !adding && (
        <div className="cv-empty">
          <p>No variables yet. Start with <code>baseUrl</code>: every request in {name} can use it as <code>{"{{baseUrl}}"}</code>.</p>
          <Button variant="primary" icon="plus" onClick={() => setAdding(true)}>Add a variable</Button>
        </div>
      )}

      {rows.length > 0 && (
        <div className="cv-grid" role="table" aria-label={`Variables of ${name}`}>
          <div className="cv-cols" role="row">
            <span role="columnheader">Default</span>
            {STAGES.map((s) => <span key={s} role="columnheader" className={cx(s === active && "is-active")}>{STAGE_LABEL[s]}{s === active && " ●"}</span>)}
          </div>
          {rows.map((row) => (
            <div key={row.name} className="cv-var" role="row">
              <div className="cv-vart">
                <code>{row.name}</code>
                {row.secret && <span className="cv-tag is-secret"><Icon name="lock-simple" /> secret</span>}
                <span className="cv-spacer" />
                <IconButton icon="trash" label={`Delete ${row.name}`} size="sm" tone="danger" onClick={() => void remove(row)} />
              </div>
              <div className="cv-cells">
                {row.secret ? <span className="cv-cell is-none" role="cell">—</span> : (
                  <button type="button" role="cell" className={cx("cv-cell", row.defaultValue === null ? "is-none" : "is-own", edit?.name === row.name && edit.column === "default" && "is-editing")} data-tip={row.defaultValue ?? "No default"} onClick={() => setEdit({ name: row.name, column: "default" })}>
                    {row.defaultValue === null ? "—" : row.defaultValue || "empty"}
                  </button>
                )}
                {STAGES.map((s) => {
                  const c = cellText(row.cells[s]);
                  return (
                    <button key={s} type="button" role="cell" disabled={readOnly(s)} className={cx("cv-cell", c.tone, needsValue(row.cells[s]) && s === active && "is-hot", edit?.name === row.name && edit.column === s && "is-editing")} data-tip={c.title} onClick={() => setEdit({ name: row.name, column: s })}>
                      {row.secret && row.cells[s].kind === "set" ? "● " : row.secret && c.tone === "is-gap" ? "○ " : c.tone === "is-inherit" ? "↳ " : ""}{c.text}
                    </button>
                  );
                })}
              </div>
              {edit?.name === row.name && <CellEditor key={`${edit.name}/${edit.column}`} slug={slug} row={row} column={edit.column} onClose={() => setEdit(null)} />}
            </div>
          ))}
        </div>
      )}

      <div className="cv-foot">
        <span className="cv-muted">Allowed hosts and TLS, per environment:</span>
        {STAGES.map((s) => <button key={s} type="button" className="cv-link" onClick={() => openEnvSettings(s, slug)}>{STAGE_LABEL[s]}</button>)}
      </div>
    </div>
  );
}

function CellEditor({ slug, row, column, onClose }: { slug: string; row: MatrixRow; column: Column; onClose: () => void }) {
  const stage = column === "default" ? null : column;
  const cell = stage ? row.cells[stage] : null;
  const initial = column === "default" ? (row.defaultValue ?? "") : cell?.kind === "value" || cell?.kind === "inherit" ? cell.value : "";
  const [value, setValue] = useState(initial);
  const [busy, setBusy] = useState(false);
  const info = useCollectionInfo(slug, true);

  const run = async (work: () => Promise<unknown>, done: string) => {
    setBusy(true);
    const ok = await attempt(work, done);
    setBusy(false);
    if (ok) onClose();
    return ok;
  };

  if (row.secret && stage) {
    return (
      <div className="cv-edit">
        <span className="cv-lbl">{row.name} in {STAGE_LABEL[stage]}{cell?.kind === "set" ? ": replace the stored value" : ""}</span>
        <SecretValueForm label={`New value for ${row.name} in ${STAGE_LABEL[stage]}`} submitLabel={cell?.kind === "set" ? "Replace" : "Set value"} onCancel={onClose} onSave={(v) => run(() => setSecret(stage, slug, row.name, v), `${row.name} saved for ${STAGE_LABEL[stage]}`)} />
        {cell?.kind === "set" && <Button size="sm" variant="ghost" icon="eraser" disabled={busy} onClick={() => void run(() => clearSecret(stage, slug, row.name), `Cleared ${row.name} in ${STAGE_LABEL[stage]}`)}>Clear value</Button>}
        <p className="cv-note"><Icon name="lock-simple" /> Stored on this machine. It never shows here again.</p>
      </div>
    );
  }

  const save = () => run(() => (stage ? setStageValue(stage, slug, row.name, value) : setDefault(slug, row.name, value)), `Saved ${row.name}`);
  const own = cell?.kind === "value";
  return (
    <form className="cv-edit" onSubmit={(e) => (e.preventDefault(), void save())} onKeyDown={(e) => e.key === "Escape" && (e.preventDefault(), e.stopPropagation(), onClose())}>
      <span className="cv-lbl">{stage ? `${row.name} in ${STAGE_LABEL[stage]}` : `Default of ${row.name}`}{stage && cell?.kind === "inherit" ? ": type to override the default" : ""}</span>
      <div className="cv-inline">
        <VarInput info={info} canDefine={false} ariaLabel={stage ? `Value of ${row.name} in ${STAGE_LABEL[stage]}` : `Default of ${row.name}`} placeholder="value, or {{another}}" value={value} onChange={setValue} />
        <Button type="submit" size="sm" variant="primary" loading={busy} disabled={value === initial && !(stage && cell?.kind === "empty")}>Save</Button>
        <Button size="sm" variant="ghost" onClick={onClose}>Cancel</Button>
      </div>
      {stage && own && <Button size="sm" variant="ghost" disabled={busy} onClick={() => void run(() => clearStageValue(stage, slug, row.name), row.defaultValue !== null ? `${row.name} uses the default in ${STAGE_LABEL[stage]}` : `Removed ${row.name} from ${STAGE_LABEL[stage]}`)}>{row.defaultValue !== null ? "Use the default" : "Remove from this environment"}</Button>}
    </form>
  );
}
