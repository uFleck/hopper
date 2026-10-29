import { useEffect, useMemo, useRef, useState, type CSSProperties, type KeyboardEvent, type ReactNode } from "react";
import { cancel, closeRequest, loadCollectionAuth, refreshScopeVars, saveRequest, scopeLayers, scopeTargets, selectDraftRun, send, editDraft, useStore } from "../store";
import { confirm, cx, envClass } from "../ui";
import { AddVariableDialog } from "./request/AddVariableDialog";
import { Banners } from "./request/Banners";
import { MissingSecretBanner } from "./request/MissingSecretBanner";
import { resolveEnv, sectionCounts, varInfo, variableOptions } from "./request/logic";
import { RequestCrumb, RequestHeader } from "./request/RequestHeader";
import { RequestSections } from "./request/RequestSections";
import { SaveDialog } from "./request/SaveDialog";
import { ResponsePane, runTone } from "./response/ResponsePane";
import "../styles/request.css";
import "../styles/collection.css";
import "../styles/response.css";

/** The request editor with its response: stacked in a narrow panel, side by side from about 640 px. */
export default function RequestView() {
  const draft = useStore((s) => s.draft);
  const stageDto = useStore((s) => s.environments.find((e) => e.active));
  const collection = useStore((s) => s.draft?.base.collection);
  const env = useMemo(() => resolveEnv(stageDto, collection), [stageDto, collection]);
  const run = useStore(selectDraftRun);
  const inheritedAuth = useStore((s) => (s.draft?.base.collection ? s.collectionAuth[s.draft.base.collection]?.auth : undefined));
  const [picking, setPicking] = useState(false);
  const [armed, setArmed] = useState(false);
  const resp = useRef<HTMLElement>(null);

  const folderKey = useStore((s) => s.draft?.base.folder.join("/") ?? "");
  const savedId = useStore((s) => (s.draft && s.draft.base.revision !== null && s.draft.doc.id ? s.draft.doc.id : null));
  const scopeVars = useStore((s) => s.scopeVars);
  const addingVariable = useStore((s) => s.addingVariable);
  const targets = useMemo(() => scopeTargets(collection, folderKey ? folderKey.split("/") : [], savedId), [collection, folderKey, savedId]);
  useEffect(() => void refreshScopeVars(), [targets]);
  useEffect(() => void (collection && loadCollectionAuth(collection)), [collection]);

  const info = useMemo(() => varInfo(env, scopeLayers(scopeVars, targets), collection ?? null), [env, scopeVars, targets, collection]);
  const variables = useMemo(() => variableOptions(info), [info]);

  const runId = draft?.runId;
  useEffect(() => {
    if (runId) resp.current?.scrollIntoView({ block: "nearest", behavior: "smooth" });
  }, [runId]);

  const needArm = env?.name === "production" && !!draft && draft.doc.method !== "GET";
  // The arm switch holds for 5 seconds, and never carries over to another request, method or environment.
  useEffect(() => {
    if (!armed) return;
    const t = setTimeout(() => setArmed(false), 5000);
    return () => clearTimeout(t);
  }, [armed]);
  useEffect(() => setArmed(false), [needArm, draft?.doc.id, draft?.doc.method, env?.name]);

  if (!draft) return null;
  const saved = draft.base.revision !== null && !!draft.doc.id;
  const readOnly = draft.base.readOnly;

  const save = async () => {
    if (readOnly || draft.saving) return;
    if (draft.base.revision === null) setPicking(true);
    else if (draft.dirty) await saveRequest();
  };

  const doSend = async () => {
    if (run && (run.phase === "preparing" || run.phase === "sending" || run.phase === "receiving")) return;
    if (needArm && !armed) return;
    setArmed(false);
    void send();
  };

  const back = async () => {
    if (draft.dirty && !(await confirm({ title: "Discard unsaved changes?", message: "This request has edits that are not saved.", confirmLabel: "Discard", tone: "danger" }))) return;
    closeRequest();
  };

  const onKeyDown = (e: KeyboardEvent) => {
    if (e.defaultPrevented) return;
    const mod = e.ctrlKey || e.metaKey;
    if (mod && e.key.toLowerCase() === "s") {
      e.preventDefault();
      void save();
    } else if (mod && e.key === "Enter") {
      e.preventDefault();
      void doSend();
    }
  };

  const inFlight = !!run && (run.phase === "preparing" || run.phase === "sending" || run.phase === "receiving");
  const tone = runTone(run);
  const sections = sectionCounts(draft.doc);

  return (
    <div className={`rv${env?.name === "production" ? " rv--prod" : ""}`} onKeyDown={onKeyDown}>
      <RequestCrumb draft={draft} onSave={() => void save()} onBack={() => void back()} />
      <div className="rv-main">
        <Banners draft={draft} />
        <MissingSecretBanner doc={draft.doc} info={info} collection={collection ?? null} inherited={inheritedAuth} onSend={() => void doSend()} busy={inFlight} />
        <div className={cx("rv-circuit", run && "is-powered", inFlight && "is-live")} style={{ "--rc": tone } as CSSProperties}>
          <span className="rv-wire" aria-hidden="true"><i className="rv-spark" /></span>

          <section className="rv-node-wrap">
            <span className="rv-node" aria-hidden="true" />
            <div className="rv-card rv-card--env">
              <i className={cx("rv-env-dot", env && envClass(env.name))} aria-hidden="true" />
              {env ? <><b>{env.name}</b> environment</> : <span>No environment</span>}
            </div>
          </section>

          <section className="rv-node-wrap">
            <span className="rv-node" aria-hidden="true" />
            <div className="rv-card">
              <RequestHeader draft={draft} env={env} info={info} variables={variables} run={run} needArm={needArm} armed={armed} onArm={() => setArmed((a) => !a)} onSend={() => void doSend()} onCancel={() => void cancel()} onSave={() => void save()} />
              <RequestDetails summary={`${sections.params} param${sections.params === 1 ? "" : "s"} · ${sections.headers} header${sections.headers === 1 ? "" : "s"} · ${draft.doc.body.type === "none" ? "no body" : `${draft.doc.body.type} body`}`}>
                <RequestSections doc={draft.doc} edit={editDraft} info={info} variables={variables} readOnly={readOnly} onSave={() => void save()} />
              </RequestDetails>
            </div>
          </section>

          <section className="rv-node-wrap" ref={resp}>
            <span className={cx("rv-node", tone !== "var(--wire)" && "is-lit")} aria-hidden="true" />
            <div className="rv-card rv-card--resp">
              <ResponsePane run={run} requestId={saved ? draft.doc.id : null} onRetry={() => void doSend()} />
            </div>
          </section>
        </div>
      </div>
      {picking && <SaveDialog onClose={() => setPicking(false)} />}
      {addingVariable && <AddVariableDialog name={addingVariable} collection={collection ?? null} />}
    </div>
  );
}

/** Params, headers and the rest stay folded until you ask: the card shows only their counts. */
function RequestDetails({ summary, children }: { summary: string; children: ReactNode }) {
  const [open, setOpen] = useState(false);
  return (
    <div className={cx("rv-disc", open && "is-open")}>
      <button type="button" className="rv-disc-head" aria-expanded={open} onClick={() => setOpen(!open)}>
        <i className="ph ph-caret-right" aria-hidden="true" />
        Details
        <span className="rv-disc-chip">{summary}</span>
      </button>
      {open && <div className="rv-disc-body">{children}</div>}
    </div>
  );
}
