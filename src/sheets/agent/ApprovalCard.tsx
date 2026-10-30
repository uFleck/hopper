import { useEffect, useState } from "react";
import type { ApprovalDecision, ApprovalRequest } from "../../ipc";
import { respondApproval } from "../../store";
import { Button, Chip, Icon, cx, envClass } from "../../ui";

/** False until `ms` after mount (or after `key` changes): the keystroke-capture guard. */
export function useArmed(ms: number, key: string): boolean {
  const [armed, setArmed] = useState(ms <= 0);
  useEffect(() => {
    if (ms <= 0) return setArmed(true);
    setArmed(false);
    const t = setTimeout(() => setArmed(true), ms);
    return () => clearTimeout(t);
  }, [ms, key]);
  return armed;
}

const DECISIONS: { decision: ApprovalDecision; label: string; hint: string; variant: "secondary" | "primary" | "danger" | "ghost" }[] = [
  { decision: "allow_once", label: "Allow once", hint: "Run this one send", variant: "secondary" },
  { decision: "allow_session", label: "Allow for this session", hint: "Until Hopper restarts", variant: "secondary" },
  { decision: "always_allow", label: "Always allow this request", hint: "Sets the request to Auto", variant: "secondary" },
  { decision: "block", label: "Block this request", hint: "Sets the request to Blocked", variant: "secondary" },
  { decision: "deny", label: "Deny", hint: "Refuse this send only", variant: "danger" },
];

/** The arm timer as eight segments that light up in turn over `armAfterMs`. */
const DUST = [0, 1, 2, 3, 4, 5, 6, 7];

/** One pending approval. No default or autofocused button; every button is disabled while arming. */
export function ApprovalCard({ approval, position }: { approval: ApprovalRequest; position?: { index: number; total: number } }) {
  const armed = useArmed(approval.armAfterMs, approval.id);
  const [busy, setBusy] = useState(false);
  const production = approval.stage === "production";
  const r = approval.request;

  const decide = async (d: ApprovalDecision) => {
    if (!armed || busy) return;
    setBusy(true);
    await respondApproval(approval.id, d);
    setBusy(false);
  };

  return (
    <article className={cx("ap-card", envClass(approval.stage), production && "is-production", !armed && "is-arming")} aria-label={`Approval for ${approval.requestName}`}>
      <div className="ap-dust" aria-hidden="true" data-armed={armed}>
        {DUST.map((i) => (
          <i key={i} style={{ animationDuration: `${approval.armAfterMs / DUST.length}ms`, animationDelay: `${(approval.armAfterMs * i) / DUST.length}ms` }} />
        ))}
      </div>
      <header className="ap-head">
        <Icon name="shield-warning" />
        <strong className="ap-title">{approval.client} wants to send a request</strong>
        {position && position.total > 1 && <span className="ap-queue">{position.index} of {position.total}</span>}
      </header>
      <div className="ap-meta">
        <span className="ap-name">{approval.requestName}</span>
        <Chip env={approval.stage}>{approval.stage}</Chip>
        <Chip>{approval.application}</Chip>
      </div>
      {production && <p className="ap-prod" role="note"><Icon name="warning" /> This is production. It will reach real systems.</p>}
      <p className="ap-reason">Why it asks: {approval.reason}</p>
      <div className="ap-req">
        <div className="ap-line"><Chip method={r.method} /><code className="ap-url">{r.url}</code></div>
        {r.headers.length > 0 && (
          <dl className="ap-headers">
            {r.headers.map(([k, v], i) => (
              <div key={i}><dt>{k}</dt><dd>{v}</dd></div>
            ))}
          </dl>
        )}
        {r.bodyPreview && <pre className="ap-body">{r.bodyPreview}</pre>}
      </div>
      <p className="ap-state" role="status" aria-live="polite">{armed ? "Choose one. Nothing is selected for you." : "Getting ready. Buttons unlock in a moment so a stray keypress cannot approve this."}</p>
      <div className="ap-actions">
        {DECISIONS.map((d) => (
          <Button key={d.decision} className={`ap-${d.decision}`} variant={d.variant} disabled={!armed || busy} data-tip={d.hint} onClick={() => void decide(d.decision)}>{d.label}</Button>
        ))}
      </div>
    </article>
  );
}
