import { useState } from "react";
import { closeSheet, useStore, type AgentTab } from "../store";
import { Sheet, Tabs } from "../ui";
import { ActivityTab } from "./agent/ActivityTab";
import { ApprovalCard } from "./agent/ApprovalCard";
import { ConnectTab } from "./agent/ConnectTab";
import { GrantsTab } from "./agent/GrantsTab";

export default function AgentSheet() {
  const initialTab = useStore((s) => s.agentTab);
  const [tab, setTab] = useState<AgentTab>(initialTab ?? "activity");
  const approvals = useStore((s) => s.approvals);
  const grantsHint = useStore((s) => s.activity.length);

  return (
    <Sheet open title="Agent" onClose={closeSheet}>
      <Tabs
        ariaLabel="Agent sections"
        value={tab}
        onChange={setTab}
        tabs={[
          { value: "activity", label: "Activity", badge: grantsHint || undefined },
          { value: "approvals", label: "Approvals", badge: approvals.length || undefined },
          { value: "grants", label: "Grants" },
          { value: "connect", label: "Connect" },
        ]}
      />
      <div className="ag-panel" role="tabpanel" aria-label={tab}>
        {tab === "activity" && <ActivityTab />}
        {tab === "approvals" && (approvals.length === 0 ? <p className="sh-empty">No approvals waiting.</p> : approvals.map((a, i) => <ApprovalCard key={a.id} approval={a} position={{ index: i + 1, total: approvals.length }} />))}
        {tab === "grants" && <GrantsTab />}
        {tab === "connect" && <ConnectTab />}
      </div>
    </Sheet>
  );
}
