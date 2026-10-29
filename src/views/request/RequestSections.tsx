import type { KeyboardEvent, ReactNode } from "react";
import type { RequestDoc } from "../../ipc";
import { Chip, Icon, cx, type VariableOption } from "../../ui";
import { AuthSection } from "./AuthSection";
import { BodySection } from "./BodySection";
import { KeyValueTable } from "./KeyValueTable";
import { pathParamNames, sectionCounts, type VarInfo } from "./logic";
import { VarInput } from "./VarInput";

export type RequestTab = "params" | "headers" | "body" | "auth" | "notes" | "source";

/** A request with a body opens on it, anything else on its params. */
export const defaultTab = (doc: RequestDoc): RequestTab => (doc.method !== "GET" && doc.body.type !== "none" ? "body" : "params");

export interface RequestSectionsProps {
  doc: RequestDoc;
  edit: (patch: Partial<RequestDoc>) => void;
  info: VarInfo;
  variables: VariableOption[];
  readOnly: boolean;
  onSave: () => void;
  tab: RequestTab;
  /** Folded: only the tab strip shows, its badges are the summary. */
  folded: boolean;
  onChange: (tab: RequestTab, folded: boolean) => void;
}

interface TabDef {
  id: RequestTab;
  label: string;
  badge?: ReactNode;
  warn?: boolean;
}

/** Params, Headers, Body, Auth, Notes and Source as one tab strip. Clicking the open tab, or the chevron, folds the pane. */
export function RequestSections({ doc, edit, info, variables, readOnly, onSave, tab, folded, onChange }: RequestSectionsProps) {
  const counts = sectionCounts(doc);
  const names = pathParamNames(doc.url);

  const tabs: TabDef[] = [
    { id: "params", label: "Params", badge: counts.params || undefined },
    { id: "headers", label: "Headers", badge: counts.headers || undefined },
    { id: "body", label: "Body", badge: doc.body.type === "none" ? undefined : doc.body.type },
    { id: "auth", label: "Auth", badge: doc.auth.type === "none" ? undefined : doc.auth.type },
    { id: "notes", label: "Notes", badge: counts.notes ? "•" : undefined },
    ...(doc.source ? [{ id: "source" as const, label: "Source", badge: doc.source.orphaned ? "orphaned" : undefined, warn: doc.source.orphaned }] : []),
  ];

  const setPathParam = (name: string, value: string) => edit({ pathParams: Object.fromEntries(names.map((n) => [n, n === name ? value : (doc.pathParams[n] ?? "")])) });

  const select = (id: RequestTab) => onChange(id, id === tab && !folded);

  const onKeyDown = (e: KeyboardEvent) => {
    const step = e.key === "ArrowRight" ? 1 : e.key === "ArrowLeft" ? -1 : 0;
    if (!step) return;
    e.preventDefault();
    const next = tabs[(tabs.findIndex((t) => t.id === tab) + step + tabs.length) % tabs.length];
    onChange(next.id, false);
    requestAnimationFrame(() => document.getElementById(`rv-tab-${next.id}`)?.focus());
  };

  return (
    <div className={cx("rv-tabs", folded && "is-folded")}>
      <div className="rv-tabbar" role="tablist" aria-label="Request sections" onKeyDown={onKeyDown}>
        {tabs.map((t) => (
          <button key={t.id} type="button" role="tab" id={`rv-tab-${t.id}`} aria-selected={t.id === tab} aria-controls="rv-pane" tabIndex={t.id === tab ? 0 : -1} className={cx("rv-tab", t.id === tab && "is-on", t.warn && "is-warn")} onClick={() => select(t.id)}>
            {t.label}
            {t.badge != null && <u className="rv-tab-badge">{t.badge}</u>}
          </button>
        ))}
        <button type="button" className="rv-fold" aria-label={folded ? "Open the section" : "Fold the section"} aria-expanded={!folded} title={folded ? "Open" : "Fold"} onClick={() => onChange(tab, !folded)}>
          <Icon name="caret-up" />
        </button>
      </div>

      {!folded && (
        <div className="rv-pane" id="rv-pane" role="tabpanel" aria-labelledby={`rv-tab-${tab}`}>
          {tab === "params" && (
            <>
              {names.length > 0 && (
                <div className="rv-pathparams">
                  <h4 className="rv-sub">Path</h4>
                  {names.map((n) => (
                    <div key={n} className="rv-pathparam">
                      <label className="rv-pathparam-name" htmlFor={`pp-${n}`}>{`{${n}}`}</label>
                      <VarInput value={doc.pathParams[n] ?? ""} info={info} ariaLabel={`Path parameter ${n}`} placeholder="value" disabled={readOnly} onChange={(v) => setPathParam(n, v)} data={{ id: `pp-${n}` }} />
                    </div>
                  ))}
                </div>
              )}
              <h4 className="rv-sub">Query</h4>
              <KeyValueTable rows={doc.query} info={info} noun="query parameter" disabled={readOnly} onChange={(query) => edit({ query })} />
            </>
          )}
          {tab === "headers" && <KeyValueTable rows={doc.headers} info={info} noun="header" disabled={readOnly} namePlaceholder="Header-Name" onChange={(headers) => edit({ headers })} />}
          {tab === "body" && <BodySection body={doc.body} info={info} variables={variables} readOnly={readOnly} onSave={onSave} onChange={(body) => edit({ body })} />}
          {tab === "auth" && <AuthSection auth={doc.auth} info={info} readOnly={readOnly} onChange={(auth) => edit({ auth })} />}
          {tab === "notes" && <textarea className="rv-notes" aria-label="Notes" value={doc.notes ?? ""} readOnly={readOnly} placeholder="What this request is for, gotchas, example ids" rows={4} onChange={(e) => edit({ notes: e.target.value || undefined })} />}
          {tab === "source" && doc.source && (
            <>
              <dl className="rv-source">
                <dt>Spec</dt>
                <dd>{doc.source.spec}</dd>
                <dt>Operation</dt>
                <dd><code>{doc.source.key}</code></dd>
              </dl>
              {doc.source.orphaned ? (
                <p className="rv-hint is-warn" role="status">
                  <Chip tone="warn">orphaned</Chip> This operation is gone from the spec after the last re-import. The request still works; check the endpoint before relying on it.
                </p>
              ) : (
                <p className="rv-hint">Created from this OpenAPI operation. Re-importing the spec flags it if the operation changes or disappears.</p>
              )}
            </>
          )}
        </div>
      )}
    </div>
  );
}
