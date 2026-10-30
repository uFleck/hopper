import type { CSSProperties, MouseEvent } from "react";
import { inTauri } from "../ipc";
import { Mascot } from "../components/Mascot";
import { useMascot } from "../components/useMascot";
import { panelDrag, panelEvent, type PanelModeInfo } from "../panel/control";
import { markNoticesSeen, openRequest, useStore, type EndpointNotice } from "../store";
import "../styles/floating.css";

/** Pressing the handle starts the OS move loop; releasing without having moved is a click, which opens the panel. */
function press(e: MouseEvent, onClick: () => void) {
  if (e.button !== 0) return;
  e.preventDefault();
  void panelDrag().then((moved) => !moved && onClick());
}

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

/** The handle on the screen edge: half out of sight until hovered, like a drawer pull. */
function Tab({ onClick }: { onClick: () => void }) {
  const mascot = useMascot();
  const { approvals, unseen, badge } = useAttention();
  const label = approvals ? `${approvals} approval${approvals === 1 ? "" : "s"} waiting` : unseen ? `${unseen} new endpoint${unseen === 1 ? "" : "s"}` : "Open Hopper";
  const lamp = approvals ? "var(--warn)" : unseen ? "var(--accent)" : undefined;
  return (
    <button
      type="button"
      className="fh-tab"
      style={lamp ? ({ "--lampc": lamp } as CSSProperties) : undefined}
      onMouseDown={(e) => press(e, onClick)}
      aria-label={label}
      data-tip={`${label}. Drag to move it to the other edge.`}
    >
      <Mascot state={mascot} size={32} />
      {badge > 0 && <span className={approvals ? "fh-badge is-warn" : "fh-badge"}>{badge}</span>}
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
      {info.mode === "compact" ? <Pill onClick={open} /> : <Tab onClick={() => open()} />}
    </div>
  );
}
