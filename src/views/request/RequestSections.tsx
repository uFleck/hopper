import type { RequestDoc } from "../../ipc";
import { Chip, type VariableOption } from "../../ui";
import { AuthSection } from "./AuthSection";
import { BodySection } from "./BodySection";
import { KeyValueTable } from "./KeyValueTable";
import { pathParamNames, sectionCounts, type VarInfo } from "./logic";
import { Section } from "./Section";
import { VarInput } from "./VarInput";

export interface RequestSectionsProps {
  doc: RequestDoc;
  edit: (patch: Partial<RequestDoc>) => void;
  info: VarInfo;
  variables: VariableOption[];
  readOnly: boolean;
  onSave: () => void;
}

/** Params, Headers, Body, Auth, Notes and Source as collapsible blocks. */
export function RequestSections({ doc, edit, info, variables, readOnly, onSave }: RequestSectionsProps) {
  const counts = sectionCounts(doc);
  const names = pathParamNames(doc.url);

  const setPathParam = (name: string, value: string) => edit({ pathParams: Object.fromEntries(names.map((n) => [n, n === name ? value : (doc.pathParams[n] ?? "")])) });

  return (
    <div className="rv-sections">
      <Section id="params" title="Params" defaultOpen badge={counts.params}>
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
      </Section>

      <Section id="headers" title="Headers" badge={counts.headers}>
        <KeyValueTable rows={doc.headers} info={info} noun="header" disabled={readOnly} namePlaceholder="Header-Name" onChange={(headers) => edit({ headers })} />
      </Section>

      <Section id="body" title="Body" defaultOpen={doc.method !== "GET"} badge={doc.body.type === "none" ? undefined : doc.body.type}>
        <BodySection body={doc.body} info={info} variables={variables} readOnly={readOnly} onSave={onSave} onChange={(body) => edit({ body })} />
      </Section>

      <Section id="auth" title="Auth" badge={doc.auth.type === "inherit" || doc.auth.type === "none" ? undefined : doc.auth.type}>
        <AuthSection auth={doc.auth} info={info} readOnly={readOnly} onChange={(auth) => edit({ auth })} />
      </Section>

      <Section id="notes" title="Notes" badge={counts.notes ? "•" : undefined}>
        <textarea className="rv-notes" aria-label="Notes" value={doc.notes ?? ""} readOnly={readOnly} placeholder="What this request is for, gotchas, example ids" rows={4} onChange={(e) => edit({ notes: e.target.value || undefined })} />
      </Section>

      {doc.source && (
        <Section id="source" title="Source" tone={doc.source.orphaned ? "warn" : undefined} badge={doc.source.orphaned ? "orphaned" : undefined}>
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
        </Section>
      )}
    </div>
  );
}
