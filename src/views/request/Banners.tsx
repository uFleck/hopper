import type { ReactNode } from "react";
import type { AgentAccess, HopperError } from "../../ipc";
import { Button, Icon, cx } from "../../ui";
import { keepMine, reloadDraft, saveRequest, setAgentAccess, type DraftState } from "../../store";

function Banner({ tone, icon, children, actions }: { tone: "warn" | "err" | "info"; icon: string; children: ReactNode; actions?: ReactNode }) {
  return (
    <div className={cx("rv-banner", `rv-banner--${tone}`)} role={tone === "info" ? "status" : "alert"}>
      <Icon name={icon} />
      <div className="rv-banner-text">{children}</div>
      {actions && <div className="rv-banner-actions">{actions}</div>}
    </div>
  );
}

const ReloadKeep = () => (
  <>
    <Button size="sm" onClick={() => void reloadDraft()}>Reload</Button>
    <Button size="sm" onClick={() => void keepMine()}>Keep mine</Button>
  </>
);

/** Everything that needs the user's attention before or after editing: read-only, conflicts, save errors, agent changes. */
export function Banners({ draft }: { draft: DraftState }) {
  const err: HopperError | null = draft.saveError;
  const saved = draft.base.revision !== null && !!draft.doc.id;
  const reallow = (a: AgentAccess) => void setAgentAccess([draft.doc.id], a);
  return (
    <>
      {draft.base.readOnly && (
        <Banner tone="info" icon="lock-simple">This file was written by a newer Hopper. You can read and send it, but not save it.</Banner>
      )}
      {draft.fromHistory === "unsaved" && (
        <Banner tone="info" icon="clock-counter-clockwise" actions={<Button size="sm" loading={draft.saving} onClick={() => void saveRequest()}>Save as request</Button>}>
          <strong>Not saved</strong> · opened from history.
        </Banner>
      )}
      {draft.fromHistory === "no-request" && (
        <Banner tone="info" icon="clock-counter-clockwise">
          This run did not keep its request, so the saved request is shown as it is now, with that run's response.
        </Banner>
      )}
      {draft.conflict && (
        <Banner tone="err" icon="git-diff" actions={<ReloadKeep />}>
          <strong>Changed on disk since you opened it.</strong> Reload to take that version and lose your edits, or keep yours and save again to overwrite.
        </Banner>
      )}
      {draft.external && !draft.conflict && (
        <Banner tone="warn" icon="arrows-clockwise" actions={<ReloadKeep />}>
          <strong>This request changed on disk</strong> (the agent, git or another editor) while you have unsaved edits.
        </Banner>
      )}
      {err && (
        <Banner tone="err" icon="warning-circle">
          <strong>Not saved.</strong> {err.message}
          {err.hint && <span className="rv-banner-hint"> {err.hint}</span>}
        </Banner>
      )}
      {saved && draft.base.grantSuspended && (
        <Banner
          tone="warn"
          icon="robot"
          actions={
            <>
              <Button size="sm" onClick={() => reallow("auto")}>Re-allow Auto</Button>
              <Button size="sm" onClick={() => reallow("ask")}>Ask each time</Button>
              <Button size="sm" onClick={() => reallow("blocked")}>Block</Button>
            </>
          }
        >
          <strong>Changed by the agent, review and re-allow.</strong> Its access was paused until you look at what changed.
        </Banner>
      )}
    </>
  );
}
