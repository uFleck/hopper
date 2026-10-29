import { useEffect, useMemo } from "react";
import type { Stage } from "../../ipc";
import { loadCollectionAuth, loadVariables, targetKey, useStore, type CollectionTab } from "../../store";
import { Button, Icon, cx } from "../../ui";
import { STAGE_LABEL, buildMatrix } from "./matrix";
import { progress, setupItems, type SetupItem } from "./setup";

/** What is still missing in a collection, computed from the variables, the stages and the saved auth. Loads what it needs. */
export function useSetup(slug: string, operations: number) {
  const defaults = useStore((s) => s.scopeVars[targetKey({ type: "collection", slug })]);
  const auth = useStore((s) => s.collectionAuth[slug]);
  const stages = useStore((s) => s.environments);
  const active: Stage | null = stages.find((s) => s.active)?.stage ?? null;

  useEffect(() => {
    void loadVariables({ type: "collection", slug });
    void loadCollectionAuth(slug);
  }, [slug]);

  const rows = useMemo(() => buildMatrix(defaults?.variables ?? [], stages, slug), [defaults, stages, slug]);
  const items = useMemo(() => setupItems({ operations, rows, auth: auth?.auth, active }), [operations, rows, auth, active]);
  return { rows, items, active, ready: defaults !== undefined && auth !== undefined };
}

function SetupRow({ item, onGo }: { item: SetupItem; onGo: (tab: CollectionTab) => void }) {
  const mark = item.state === "ready" ? "✓" : item.state === "todo" ? "!" : "";
  return (
    <li className={cx("cs-item", `is-${item.state}`)}>
      <span className="cs-dot" aria-hidden="true">{mark}</span>
      <span className="cs-text"><b>{item.title}</b> <span className="cv-muted">{item.detail}</span></span>
      {item.go && <Button size="sm" variant={item.state === "todo" ? undefined : "ghost"} className={item.state === "todo" ? "cs-warnbtn" : undefined} onClick={() => onGo(item.go!.tab)}>{item.go.label}</Button>}
    </li>
  );
}

const TAB_LABEL: Record<CollectionTab, string> = { requests: "Requests", variables: "Variables", auth: "Auth", specs: "Specs" };

/** The setup checklist and the tab bar of a collection page. The caller draws the selected tab under it. */
export function CollectionTabs({ slug, operations, requests, specs, tab, onTab }: { slug: string; operations: number; requests: number; specs: number; tab: CollectionTab; onTab: (t: CollectionTab) => void }) {
  const { rows, items, active, ready } = useSetup(slug, operations);
  const { done, total } = progress(items);
  const authTodo = items.some((i) => i.key === "auth" && i.state === "todo");
  const counts: Record<CollectionTab, number> = { requests, variables: rows.length, auth: 0, specs };

  return (
    <>
      {ready && (
        <section className="cs-card" aria-label="Setup">
          <div className="cs-head">
            <b>Setup</b>
            <span className="cv-muted">{done} of {total} ready{active ? ` in ${STAGE_LABEL[active]}` : ""}</span>
          </div>
          <div className="cs-bar" role="progressbar" aria-valuemin={0} aria-valuemax={total} aria-valuenow={done}><i style={{ width: `${total ? (done / total) * 100 : 0}%` }} /></div>
          <ul className="cs-list">
            {items.map((i) => <SetupRow key={i.key} item={i} onGo={onTab} />)}
          </ul>
          {!active && <p className="cv-muted"><Icon name="info" /> No environment is active, so values are not checked. Pick one in Environments.</p>}
        </section>
      )}
      <div className="cs-tabs" role="tablist" aria-label="Collection">
        {(Object.keys(TAB_LABEL) as CollectionTab[]).map((t) => (
          <button key={t} type="button" role="tab" aria-selected={tab === t} className={cx(tab === t && "is-on")} onClick={() => onTab(t)}>
            {TAB_LABEL[t]}
            {counts[t] > 0 && <i>{counts[t]}</i>}
            {t === "auth" && authTodo && <i className="is-warn" aria-label="needs attention">!</i>}
          </button>
        ))}
      </div>
    </>
  );
}
