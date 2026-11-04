import { useState } from "react";
import type { WorkspaceDeleteRequest } from "../../ipc";
import { respondWorkspaceDelete } from "../../store";
import { Button, Icon, cx } from "../../ui";
import { useArmed } from "./ApprovalCard";

/** The arm timer as eight segments, the same as an approval. */
const DUST = [0, 1, 2, 3, 4, 5, 6, 7];

/** One pending request to delete a workspace. No default or autofocused button; both buttons are disabled while arming. */
export function WorkspaceDeleteCard({ request, position }: { request: WorkspaceDeleteRequest; position?: { index: number; total: number } }) {
  const armed = useArmed(request.armAfterMs, request.id);
  const [busy, setBusy] = useState(false);

  const decide = async (approve: boolean) => {
    if (!armed || busy) return;
    setBusy(true);
    await respondWorkspaceDelete(request.id, approve);
    setBusy(false);
  };

  return (
    <article className={cx("ap-card", !armed && "is-arming")} aria-label={`Workspace deletion for ${request.name}`}>
      <div className="ap-dust" aria-hidden="true" data-armed={armed}>
        {DUST.map((i) => (
          <i key={i} style={{ animationDuration: `${request.armAfterMs / DUST.length}ms`, animationDelay: `${(request.armAfterMs * i) / DUST.length}ms` }} />
        ))}
      </div>
      <header className="ap-head">
        <Icon name="shield-warning" />
        <strong className="ap-title">{request.client} wants to delete a workspace</strong>
        {position && position.total > 1 && <span className="ap-queue">{position.index} of {position.total}</span>}
      </header>
      <div className="ap-meta">
        <span className="ap-name">{request.name}</span>
      </div>
      <div className="ap-req">
        <div className="ap-line"><code className="ap-url">{request.path}</code></div>
      </div>
      <p className="ap-reason">Removes its collections, environments and hopper.yaml, and Hopper's local data for it. The folder and its other files stay. This cannot be undone.</p>
      <p className="ap-state" role="status" aria-live="polite">{armed ? "Choose one. Nothing is selected for you." : "Getting ready. Buttons unlock in a moment so a stray keypress cannot approve this."}</p>
      <div className="ap-actions">
        <Button className="ap-delete" variant="danger" disabled={!armed || busy} data-tip="Delete this workspace from Hopper" onClick={() => void decide(true)}>Delete workspace</Button>
        <Button className="ap-keep" variant="secondary" disabled={!armed || busy} data-tip="Leave the workspace as it is" onClick={() => void decide(false)}>Keep it</Button>
      </div>
    </article>
  );
}
