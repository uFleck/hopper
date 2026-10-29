import { useMemo, useState } from "react";
import type { Stage, StageDto } from "../../ipc";
import { openCollection, useStore } from "../../store";
import { Button, Chip, Icon, TextInput } from "../../ui";
import { attempt } from "../collection/controls";
import { groupByApplication, secretGaps, STAGE_LABEL } from "../collection/matrix";
import { setSecret } from "../collection/ops";

/** What is missing in one stage, grouped by collection: the open collection first and expanded, the rest collapsed, finished ones hidden. */
export function ReviewList({ stage, environments }: { stage: Stage; environments: StageDto[] }) {
  const current = useStore((s) => s.draft?.base.collection ?? null);
  const [filter, setFilter] = useState("");
  const [open, setOpen] = useState<Set<string> | null>(null);
  const [values, setValues] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState<string | null>(null);
  const [showDone, setShowDone] = useState(false);
  const readOnly = !!environments.find((e) => e.stage === stage)?.readOnly;

  const gaps = useMemo(() => secretGaps(environments, stage), [environments, stage]);
  const groups = useMemo(() => {
    const all = groupByApplication(gaps).sort((a, b) => (a.application === current ? -1 : b.application === current ? 1 : a.application.localeCompare(b.application)));
    const q = filter.trim().toLowerCase();
    return q ? all.filter((g) => g.application.toLowerCase().includes(q) || g.gaps.some((x) => x.name.toLowerCase().includes(q))) : all;
  }, [gaps, current, filter]);
  const done = useMemo(() => {
    const withGaps = new Set(gaps.map((g) => g.application));
    return [...new Set(environments.flatMap((e) => e.applications.map((a) => a.application)))].filter((a) => !withGaps.has(a)).sort();
  }, [environments, gaps]);

  const expanded = open ?? new Set(groups.slice(0, 1).map((g) => g.application));
  const toggle = (app: string) => setOpen((o) => {
    const next = new Set(o ?? expanded);
    if (!next.delete(app)) next.add(app);
    return next;
  });
  const key = (app: string, name: string) => `${app}/${name}`;

  const saveGroup = async (application: string, names: string[]) => {
    const todo = names.filter((n) => values[key(application, n)]);
    if (!todo.length) return;
    setBusy(application);
    for (const n of todo) {
      const ok = await attempt(() => setSecret(stage, application, n, values[key(application, n)]));
      if (ok) setValues((v) => ({ ...v, [key(application, n)]: "" }));
    }
    setBusy(null);
  };

  if (gaps.length === 0) {
    return <p className="cv-muted">Every secret has a value in {STAGE_LABEL[stage]}.</p>;
  }

  return (
    <div className="cv">
      <TextInput aria-label="Filter collections" placeholder="Filter collections or secrets" value={filter} onChange={(e) => setFilter(e.target.value)} />
      {groups.map((g) => {
        const isOpen = expanded.has(g.application);
        const filled = g.gaps.filter((x) => values[key(g.application, x.name)]).length;
        return (
          <section key={g.application} className="ev-group">
            <button type="button" className="ev-grouphead" aria-expanded={isOpen} onClick={() => toggle(g.application)}>
              <Icon name={isOpen ? "caret-down" : "caret-right"} />
              <b>{g.application}</b>
              {g.application === current && <Chip tone="accent">open now</Chip>}
              <Chip tone="warn">{g.gaps.length} missing</Chip>
            </button>
            {isOpen && (
              <form className="ev-items" onSubmit={(e) => (e.preventDefault(), void saveGroup(g.application, g.gaps.map((x) => x.name)))}>
                {g.gaps.map((x) => (
                  <label key={x.name} className="ev-item">
                    <code>{x.name}</code>
                    <TextInput type="password" mono autoComplete="new-password" disabled={readOnly} aria-label={`Value of ${x.name} in ${g.application}`} placeholder="Paste the value" value={values[key(g.application, x.name)] ?? ""} onChange={(e) => setValues({ ...values, [key(g.application, x.name)]: e.target.value })} />
                  </label>
                ))}
                <div className="cv-actions">
                  <Button variant="ghost" size="sm" onClick={() => openCollection(g.application, "variables")}>Open collection</Button>
                  <Button type="submit" size="sm" variant="primary" loading={busy === g.application} disabled={filled === 0 || readOnly}>{filled > 1 ? `Save ${filled}` : "Save"}</Button>
                </div>
              </form>
            )}
          </section>
        );
      })}
      {groups.length === 0 && <p className="cv-muted">Nothing matches “{filter}”.</p>}
      {done.length > 0 && (
        <p className="cv-muted">
          {done.length} collection{done.length === 1 ? " is" : "s are"} complete and hidden.{" "}
          <button type="button" className="cv-link" onClick={() => setShowDone(!showDone)}>{showDone ? "Hide" : "Show all"}</button>
        </p>
      )}
      {showDone && done.map((a) => <button key={a} type="button" className="cv-link" onClick={() => openCollection(a, "variables")}>{a}</button>)}
    </div>
  );
}
