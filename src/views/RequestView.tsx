import { useEffect, useMemo, useRef, useState, type CSSProperties, type KeyboardEvent } from "react";
import { cancel, closeRequest, loadCollectionAuth, refreshScopeVars, saveRequest, scopeLayers, scopeTargets, selectDraftRun, send, editDraft, useStore } from "../store";
import { confirm, cx } from "../ui";
import { AddVariableDialog } from "./request/AddVariableDialog";
import { Banners } from "./request/Banners";
import { MissingSecretBanner } from "./request/MissingSecretBanner";
import { resolveEnv, varInfo, variableOptions } from "./request/logic";
import { RequestHeader } from "./request/RequestHeader";
import { RequestSections, defaultTab, type RequestTab } from "./request/RequestSections";
import { SaveDialog } from "./request/SaveDialog";
import { ResponsePane, runTone } from "./response/ResponsePane";
import "../styles/request.css";
import "../styles/collection.css";
import "../styles/response.css";

/** The request editor with its response: one bar, a tab strip that folds once the request has been sent, then the response. */
export default function RequestView() {
  const draft = useStore((s) => s.draft);
  const stageDto = useStore((s) => s.environments.find((e) => e.active));
  const collection = useStore((s) => s.draft?.base.collection);
  const env = useMemo(() => resolveEnv(stageDto, collection), [stageDto, collection]);
  const run = useStore(selectDraftRun);
  const inheritedAuth = useStore((s) => (s.draft?.base.collection ? s.collectionAuth[s.draft.base.collection]?.auth : undefined));
  const [picking, setPicking] = useState(false);
  const [armed, setArmed] = useState(false);
  const [tab, setTab] = useState<RequestTab>("params");
  const [folded, setFolded] = useState(false);
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

  // Sending folds the tab strip so the response gets the room. Opening another request starts from its own state.
  const docId = draft?.doc.id;
  const docRef = useRef(draft?.doc);
  docRef.current = draft?.doc;
  const prevDocId = useRef(docId);
  useEffect(() => {
    if (prevDocId.current && prevDocId.current !== docId && docRef.current) {
      setTab(defaultTab(docRef.current));
      setFolded(!!runId);
    }
    prevDocId.current = docId;
  }, [docId, runId]);
  useEffect(() => {
    if (runId) setFolded(true);
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

  return (
    <div className={`rv${env?.name === "production" ? " rv--prod" : ""}`} onKeyDown={onKeyDown}>
      <RequestHeader draft={draft} env={env} info={info} variables={variables} run={run} needArm={needArm} armed={armed} onArm={() => setArmed((a) => !a)} onSend={() => void doSend()} onCancel={() => void cancel()} onSave={() => void save()} onBack={() => void back()} />
      <div className="rv-main">
        <Banners draft={draft} />
        <MissingSecretBanner doc={draft.doc} info={info} collection={collection ?? null} inherited={inheritedAuth} onSend={() => void doSend()} busy={inFlight} />
        <RequestSections doc={draft.doc} edit={editDraft} info={info} variables={variables} readOnly={readOnly} onSave={() => void save()} tab={tab} folded={folded} onChange={(t, f) => (setTab(t), setFolded(f))} />
        <section ref={resp} className={cx("rv-card", tone !== "var(--wire)" && "is-lit")} style={{ "--rc": tone } as CSSProperties}>
          <ResponsePane run={run} requestId={saved ? draft.doc.id : null} onRetry={() => void doSend()} />
        </section>
      </div>
      {picking && <SaveDialog onClose={() => setPicking(false)} />}
      {addingVariable && <AddVariableDialog name={addingVariable} collection={collection ?? null} />}
    </div>
  );
}
