import { useEffect, useRef } from "react";
import { useStore } from "../store";
import { ApprovalCard } from "../sheets/agent/ApprovalCard";
import "../styles/approval.css";

/** The in-panel approval tray, above everything. Not modal. Esc does nothing (the tray shakes); a decision is always an explicit click. */
export default function ApprovalPrompt() {
  const approvals = useStore((s) => s.approvals);
  const first = approvals[0];
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!first) return;
    const swallowEsc = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.preventDefault();
      e.stopPropagation();
      // Restart the shake: drop the class, force a reflow, add it back.
      const tray = ref.current;
      if (!tray) return;
      tray.classList.remove("shake");
      void tray.offsetWidth;
      tray.classList.add("shake");
    };
    window.addEventListener("keydown", swallowEsc, true);
    return () => window.removeEventListener("keydown", swallowEsc, true);
  }, [first?.id]);

  if (!first) return null;
  return (
    <div className="ap-layer">
      <div ref={ref} className="ap-dialog" role="alertdialog" aria-label="Approval needed" onMouseDown={(e) => e.stopPropagation()}>
        <ApprovalCard key={first.id} approval={first} position={{ index: 1, total: approvals.length }} />
      </div>
    </div>
  );
}
