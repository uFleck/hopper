import { useMemo, useState } from "react";
import { toHopperError, type Stage, type StageDto } from "../ipc";
import { closeSheet, refreshEnvironments, setActiveEnv, store, toast, useStore } from "../store";
import { Button, Chip, Icon, Sheet, TextInput, confirm, cx, envClass } from "../ui";
import { deleteEnvironment, saveEnvironment } from "./env/api";
import { blankDraft, draftFromApp, draftToInput, hasErrors, isDirty, missingSecrets, validateDraft, type EnvDraft } from "./env/draft";
import { EnvEditor } from "./env/EnvEditor";
import { LegacyBanner } from "./env/Legacy";
import { MissingSecretsList } from "./env/MissingSecrets";

interface EditView {
  draft: EnvDraft;
  base: EnvDraft;
  conflict: boolean;
  fill?: string;
  /** Remounts the editor when the draft is replaced wholesale (save, reload). */
  key: number;
}

export default function EnvironmentsSheet() {
  const environments = useStore((s) => s.environments);
  const collections = useStore((s) => s.tree?.collections ?? []);
  const [stageName, setStageName] = useState<Stage | null>(null);
  const [view, setView] = useState<EditView | null>(null);
  const [newApp, setNewApp] = useState("");
  const [saving, setSaving] = useState(false);

  const missing = useMemo(() => missingSecrets(environments), [environments]);
  const stage: StageDto | undefined = environments.find((e) => e.stage === stageName);
  const apps = stage?.applications.map((a) => a.application) ?? [];
  const live = view && stage && !view.draft.isNew ? stage.applications.find((a) => a.application === view.draft.application) : undefined;
  const errors = useMemo(() => (view ? validateDraft(view.draft, apps) : null), [view, apps.join("\n")]);
  const dirty = !!view && isDirty(view.draft, view.base);

  const open = (draft: EnvDraft, fill?: string) => setView((v) => ({ draft, base: draft, conflict: false, fill, key: (v?.key ?? 0) + 1 }));
  const openApp = (st: StageDto, application: string, fill?: string) => {
    const a = st.applications.find((x) => x.application === application);
    setStageName(st.stage);
    open(a ? draftFromApp(st, a) : { ...blankDraft(st.stage, application), expectedRevision: st.revision ?? undefined }, fill);
  };

  const back = async () => {
    if (view) {
      if (dirty && !(await confirm({ title: "Discard your changes?", message: `You have unsaved changes to ${view.draft.stage}/${view.draft.application || "a new application"}.`, confirmLabel: "Discard", tone: "danger" }))) return;
      return setView(null);
    }
    if (stageName) return setStageName(null);
    closeSheet();
  };

  const save = async () => {
    if (!view || !errors || hasErrors(errors)) return;
    setSaving(true);
    try {
      await saveEnvironment(draftToInput(view.draft));
      await refreshEnvironments();
      toast(`${view.draft.stage}/${view.draft.application.trim()} saved`, "success");
      const fresh = store.getState().environments.find((e) => e.stage === view.draft.stage);
      if (fresh) openApp(fresh, view.draft.application.trim());
    } catch (e) {
      const err = toHopperError(e);
      if (err.code === "revision_conflict") setView({ ...view, conflict: true });
      else toast(err.code === "read_only" ? "This stage is read-only: it comes from a newer Hopper." : err.message, "error");
    } finally {
      setSaving(false);
    }
  };

  const reload = async () => {
    if (!view) return;
    await refreshEnvironments();
    const fresh = store.getState().environments.find((e) => e.stage === view.draft.stage);
    if (fresh && fresh.applications.some((a) => a.application === view.draft.application)) openApp(fresh, view.draft.application);
    else setView(null);
  };

  const keepMine = async () => {
    if (!view) return;
    await refreshEnvironments();
    const fresh = store.getState().environments.find((e) => e.stage === view.draft.stage);
    setView({ ...view, conflict: false, draft: { ...view.draft, expectedRevision: fresh?.revision ?? undefined } });
  };

  const remove = async () => {
    if (!view) return;
    const { stage: st, application } = view.draft;
    const ok = await confirm({
      title: `Delete ${st}/${application}?`,
      message: `${st === "production" ? "This is production. " : ""}The section is removed from the stage file and its secret values are forgotten on this machine. Requests are not deleted.`,
      confirmLabel: "Delete section",
      tone: "danger",
    });
    if (!ok) return;
    try {
      await deleteEnvironment({ stage: st, application });
      await refreshEnvironments();
      toast(`${st}/${application} deleted`, "info");
      setView(null);
    } catch (e) {
      toast(toHopperError(e).message, "error");
    }
  };

  const editing = view && errors && stage;
  const title = editing ? `${view.draft.stage}/${view.draft.application || "new application"}` : stage ? stage.stage : "Environments";
  return (
    <Sheet
      open
      title={title}
      onClose={() => void back()}
      footer={
        editing ? (
          <>
            <Button onClick={() => void back()}>{dirty ? "Cancel" : "Close"}</Button>
            <Button variant="primary" loading={saving} disabled={!dirty || hasErrors(errors) || stage.readOnly} onClick={() => void save()}>
              {view.draft.isNew ? "Create section" : "Save changes"}
            </Button>
          </>
        ) : undefined
      }
    >
      <datalist id="env-apps">{collections.map((c) => <option key={c.slug} value={c.slug} />)}</datalist>
      {editing ? (
        <EnvEditor key={view.key} draft={view.draft} onChange={(draft) => setView({ ...view, draft })} stage={stage} live={live} errors={errors} conflict={view.conflict} initialFill={view.fill} onReload={() => void reload()} onKeepMine={() => void keepMine()} onDelete={() => void remove()} />
      ) : stage ? (
        <div className="sh-envlist">
          {stage.readOnly && <p className="sh-help">This stage file was written by a newer Hopper: read-only here.</p>}
          {stage.applications.length === 0 && <p className="sh-empty">No applications in this stage yet.</p>}
          <ul className="sh-envs">
            {stage.applications.map((a) => {
              const miss = a.secrets.filter((s) => s.status === "missing").length;
              return (
                <li key={a.application} className={cx("sh-env", envClass(stage.stage))}>
                  <button type="button" className="sh-env-open" onClick={() => openApp(stage, a.application)} aria-label={`Edit ${a.application}`}>
                    <span className="sh-env-name">{a.application}</span>
                    {miss > 0 && <Chip tone="warn" title="Secrets without a value on this machine">{miss} missing</Chip>}
                    <Icon name="caret-right" className="sh-env-go" />
                  </button>
                </li>
              );
            })}
          </ul>
          {!stage.readOnly && (
            <div className="sh-row-actions">
              <TextInput list="env-apps" aria-label="New application" placeholder="application (collection)" value={newApp} onChange={(e) => setNewApp(e.target.value)} />
              <Button icon="plus" disabled={!newApp.trim()} onClick={() => { openApp(stage, newApp.trim()); setNewApp(""); }}>Add application</Button>
            </div>
          )}
        </div>
      ) : (
        <div className="sh-envlist">
          <LegacyBanner />
          <MissingSecretsList items={missing} onPick={(m) => { const st = environments.find((x) => x.stage === m.stage); if (st) openApp(st, m.application, m.name); }} />
          <ul className="sh-envs">
            {environments.map((e) => {
              const miss = e.applications.flatMap((a) => a.secrets).filter((s) => s.status === "missing").length;
              return (
                <li key={e.stage} className={cx("sh-env", envClass(e.stage), e.active && "is-active")}>
                  <button type="button" className="sh-env-open" onClick={() => setStageName(e.stage)} aria-label={`Open ${e.stage}`}>
                    <Chip env={e.stage} />
                    <span className="sh-env-name">{e.stage}</span>
                    {e.active && <Chip tone="accent"><Icon name="check" /> active</Chip>}
                    {miss > 0 && <Chip tone="warn" title="Secrets without a value on this machine">{miss} missing</Chip>}
                    {e.readOnly && <Chip>read-only</Chip>}
                    <Icon name="caret-right" className="sh-env-go" />
                  </button>
                  {!e.active && <Button size="sm" onClick={() => void setActiveEnv(e.stage)} aria-label={`Use ${e.stage}`}>Use</Button>}
                </li>
              );
            })}
          </ul>
        </div>
      )}
    </Sheet>
  );
}
