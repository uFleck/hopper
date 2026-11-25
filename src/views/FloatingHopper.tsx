import { useEffect, useState, type CSSProperties } from "react";
import { inTauri } from "../ipc";
import { Mascot } from "../components/Mascot";
import { useMascot } from "../components/useMascot";
import { panelDrag, panelEvent, panelGrip, type PanelModeInfo } from "../panel/control";
import { markNoticesSeen, openRequest, useStore, type EndpointNotice } from "../store";
import "../styles/floating.css";

function open(notice?: EndpointNotice) {
  markNoticesSeen();
  if (notice?.requestId) void openRequest(notice.requestId);
  panelEvent("open");
}

/** What needs attention: a decision waiting, then what an agent just added. */
function useAttention() {
  const approvals = useStore((s) => s.approvals.length);
  const unseen = useStore((s) => s.notices.filter((n) => !n.seen).length);
  return { approvals, unseen, badge: approvals || unseen };
}

/** The grip expands into the desktop from its docked edge. */
function Tab({ onClick }: { onClick: () => void }) {
  const mascot = useMascot();
  const [hovered, setHovered] = useState(false);
  const [focused, setFocused] = useState(false);
  const [dragging, setDragging] = useState(false);
  const { approvals, unseen, badge } = useAttention();
  const expanded = hovered || focused || dragging;
  const label = approvals ? `${approvals} approval${approvals === 1 ? "" : "s"} waiting` : unseen ? `${unseen} new endpoint${unseen === 1 ? "" : "s"}` : "Open Hopper";
  useEffect(() => panelGrip(expanded), [expanded, dragging]);

  return (
    <button
      type="button"
      className={expanded ? "fh-tab is-expanded" : "fh-tab"}
      onPointerEnter={() => setHovered(true)}
      onPointerLeave={() => setHovered(false)}
      onFocus={(e) => setFocused(e.currentTarget.matches(":focus-visible"))}
      onBlur={() => setFocused(false)}
      onMouseDown={(e) => {
        if (e.button !== 0) return;
        e.preventDefault();
        setDragging(true);
        void panelDrag().then((moved) => {
          if (moved) setHovered(false);
          else onClick();
        }, () => setHovered(false)).finally(() => setDragging(false));
      }}
      onClick={(e) => { if (e.detail === 0) onClick(); }}
      aria-label={label}
    >
      <span className="fh-tab-nub" aria-hidden="true" />
      <Mascot state={mascot} size={32} approvalMark={false} />
      {badge > 0 && <span className={approvals ? "fh-badge is-warn" : "fh-badge"} aria-hidden="true" />}
    </button>
  );
}

/** What the pill says: a decision waiting, then what an agent just added, then what is going on. */
function usePill(): { title: string; detail: string; lamp: string; notice?: EndpointNotice } {
  const approvals = useStore((s) => s.approvals);
  const notices = useStore((s) => s.notices);
  const unseen = notices.filter((n) => !n.seen);
  const mascot = useMascot();
  const last = useStore((s) => s.activity[0]);
  if (approvals.length) {
    const a = approvals[0];
    return { title: "Needs your approval", detail: `${a.client} wants to send ${a.requestName} to ${a.stage} (${a.application})`, lamp: "var(--warn)" };
  }
  if (unseen.length) {
    const n = unseen[0];
    const more = unseen.length > 1 ? ` and ${unseen.length - 1} more` : "";
    const what = n.kind === "spec" ? `imported ${n.label} (${n.count} endpoints)` : `added ${n.label}`;
    return { title: "New endpoint", detail: `${n.client} ${what}${more}`, lamp: "var(--accent)", notice: n };
  }
  if (mascot === "sending") return { title: "Sending", detail: "A request is in flight", lamp: "var(--accent)" };
  if (last) return { title: last.client, detail: `${last.tool.replace(/^hopper_/, "")} ${last.target}`, lamp: "var(--ok)" };
  return { title: "Hopper", detail: "Ready", lamp: "var(--glow)" };
}

/** The notice pill at the top corner, shown while something needs attention. */
function Pill({ onClick }: { onClick: (notice?: EndpointNotice) => void }) {
  const { title, detail, lamp, notice } = usePill();
  return (
    <div className="fh-pill" role="status">
      <button type="button" className="fh-pill-main" onClick={() => onClick(notice)}>
        <span className="fh-bulb" style={{ "--c": lamp } as CSSProperties} aria-hidden="true" />
        <span className="fh-pill-text">
          <strong className="fh-pill-title">{title}</strong>
          <span className="fh-pill-detail">{detail}</span>
        </span>
      </button>
      <button
        type="button"
        className="fh-pill-close"
        aria-label="Dismiss"
        onClick={() => {
          markNoticesSeen();
          panelEvent("dismiss");
        }}
      >
        <i className="ph ph-x" aria-hidden="true" />
      </button>
    </div>
  );
}

/** Hopper collapsed: the handle alone (`tab`), or the notice pill (`compact`), stuck to the edge it was docked on. */
export default function FloatingHopper({ info }: { info: PanelModeInfo }) {
  const side = info.right ? "is-right" : "is-left";
  // In a plain browser there is no window around the handle, so it is placed on the page like it would be on screen.
  const preview = inTauri() ? "" : " is-preview";
  return (
    <div className={`fh ${side} fh--${info.mode}${preview}`}>
      {info.mode === "compact" ? <Pill onClick={open} /> : <Tab key={side} onClick={() => open()} />}
    </div>
  );
}
