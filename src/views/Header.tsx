import { useState, type MouseEvent } from "react";
import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { inTauri } from "../ipc";
import { Mascot } from "../components/Mascot";
import { useMascot } from "../components/useMascot";
import { panelEvent } from "../panel/control";
import { closeWorkspace, openSheet, selectActiveEnv, setActiveEnv, useStore } from "../store";
import { Icon, IconButton } from "../ui";
import { EnvPicker } from "./EnvPicker";
import { Menu, type MenuItem } from "./find/Menu";
import UpdateButton from "./UpdateButton";
import "../styles/header.css";

/** The mascot is the drag handle: the backend runs the OS move loop, then snaps the panel to the nearest screen edge. */
function startDrag(e: MouseEvent) {
  if (e.button !== 0 || !inTauri()) return;
  e.preventDefault();
  void tauriInvoke("panel_drag");
}

export default function Header() {
  const workspace = useStore((s) => s.workspace);
  const ready = useStore((s) => s.ready);
  const approvals = useStore((s) => s.approvals.length);
  const hasWorkspace = !!workspace;
  const active = useStore(selectActiveEnv);
  const mascot = useMascot();
  const [pinned, setPinned] = useState(false);

  const menu: MenuItem[] = [
    { key: "collections", label: "Collections", icon: "folders", onSelect: () => openSheet("collections") },
    { key: "environments", label: "Environments", icon: "sliders-horizontal", onSelect: () => openSheet("environments") },
    {
      key: "agent",
      label: "Agent access",
      icon: "robot",
      onSelect: () => openSheet("agent"),
      trailing: approvals > 0 ? <span className="hdr-badge" aria-label={`${approvals} pending`}>{approvals}</span> : undefined,
    },
    { key: "settings", label: "Settings", icon: "gear", onSelect: () => openSheet("settings") },
    ...(active ? [{ key: "clear-env", label: "Clear environment", icon: "prohibit", onSelect: () => void setActiveEnv(null) }] : []),
    { key: "switch", label: "Switch workspace", icon: "swap", separated: true, onSelect: () => void closeWorkspace() },
  ];

  return (
    <header className="panel-header hdr">
      <span className="hdr-drag" onMouseDown={startDrag} data-tip="Drag to the other screen edge">
        <Mascot state={mascot} />
      </span>
      <div className="panel-title">
        <span className="hdr-brand">Hopper</span>
        <span className="workspace-name">{workspace?.name ?? (ready ? "No workspace" : "Loading")}</span>
      </div>
      <UpdateButton />
      {hasWorkspace && <EnvPicker />}
      <IconButton
        icon="push-pin"
        label={pinned ? "Unpin panel" : "Pin panel open"}
        active={pinned}
        onClick={() => {
          setPinned((p) => !p);
          panelEvent("toggle-pin");
        }}
      />
      <IconButton icon="minus" label="Minimize to Hopper" onClick={() => panelEvent("dismiss")} />
      <Menu
        label="Menu"
        items={hasWorkspace ? menu : menu.filter((m) => m.key === "agent" || m.key === "settings")}
        trigger={(props) => (
          <span className="hdr-menu-btn">
            <button {...props} type="button" className="ui-iconbtn" aria-label="Menu" data-tip="Menu">
              <Icon name="list" />
            </button>
            {approvals > 0 && <span className="hdr-warn-dot hdr-menu-dot" aria-hidden="true" />}
          </span>
        )}
      />
    </header>
  );
}
