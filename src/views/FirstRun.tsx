import { Mascot } from "../components/Mascot";
import { useEffect, useState, type FormEvent } from "react";
import { initWorkspace, openAgentSheet, openWorkspace, pickAndOpenWorkspace, useStore } from "../store";
import { invoke, type DiscoveredWorkspace } from "../ipc";
import { Button, Field, Icon, TextInput } from "../ui";
import { nameFromPath } from "./find/model";
import SpecImportForm from "./find/SpecImportForm";
import "../styles/find.css";

function CreateWorkspace({ onCancel }: { onCancel: () => void }) {
  const [path, setPath] = useState("");
  const [name, setName] = useState("");
  const [touched, setTouched] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (!path.trim()) return setError("Enter the folder for the workspace.");
    const finalName = name.trim() || nameFromPath(path);
    if (!finalName) return setError("Give the workspace a name.");
    setError(null);
    setBusy(true);
    await initWorkspace(path.trim(), finalName);
    setBusy(false);
  };

  return (
    <form className="firstrun-form" onSubmit={(e) => void submit(e)}>
      <Field label="Folder" hint="Requests are stored as files in this folder, so you can commit them." error={error}>
        {(id) => (
          <TextInput
            id={id}
            mono
            autoFocus
            value={path}
            invalid={!!error}
            placeholder="C:\Work\my-apis"
            onChange={(e) => {
              setPath(e.target.value);
              if (!touched) setName(nameFromPath(e.target.value));
            }}
          />
        )}
      </Field>
      <Field label="Workspace name">
        {(id) => (
          <TextInput
            id={id}
            value={name}
            onChange={(e) => {
              setTouched(true);
              setName(e.target.value);
            }}
          />
        )}
      </Field>
      <div className="import-actions">
        <Button onClick={onCancel}>Cancel</Button>
        <Button type="submit" variant="primary" icon="folder-plus" loading={busy}>Create workspace</Button>
      </div>
    </form>
  );
}

function WorkspaceList({ label, title, items }: { label: string; title: string; items: { path: string; name: string }[] }) {
  return (
    <section aria-label={label}>
      <h2 className="recent-title">{title}</h2>
      <ul className="firstrun-list">
        {items.map((w) => (
          <li key={w.path}>
            <button type="button" className="firstrun-ws" onClick={() => void openWorkspace(w.path)}>
              <Icon name="folder" />
              <span className="firstrun-ws-text">
                <span className="firstrun-ws-name">{w.name}</span>
                <span className="firstrun-ws-path">{w.path}</span>
              </span>
            </button>
          </li>
        ))}
      </ul>
    </section>
  );
}

/** Shown while no workspace is open: pick a folder, reopen a recent one, or create a new one. */
export default function FirstRun() {
  const recentWorkspaces = useStore((s) => s.recentWorkspaces);
  const [creating, setCreating] = useState(false);
  const [discovered, setDiscovered] = useState<DiscoveredWorkspace[]>([]);

  useEffect(() => {
    let live = true;
    // Saved ones show at once; the scan then adds new ones and saves them.
    const apply = (d: DiscoveredWorkspace[]) => live && setDiscovered(d);
    invoke("workspace_known")
      .then(apply)
      .catch(() => {})
      .then(() => invoke("workspace_discover"))
      .then(apply, () => {});
    return () => {
      live = false;
    };
  }, []);
  const known = new Set(recentWorkspaces.map((w) => w.path.toLowerCase()));
  const found = discovered.filter((d) => !known.has(d.path.toLowerCase()));

  return (
    <div className="firstrun">
      <div className="firstrun-hero">
        <div className="firstrun-wm">
          <Mascot state="sleeping" size={52} />
          <span>Hopper</span>
        </div>
        <h1 className="firstrun-title">Open a workspace</h1>
        <p className="firstrun-sub">A workspace is a folder that holds your collections, environments and imported specs.</p>
        <div className="firstrun-actions">
          <Button variant="primary" icon="folder-open" onClick={() => void pickAndOpenWorkspace()}>Open folder</Button>
          <Button icon="folder-plus" onClick={() => setCreating((c) => !c)} aria-expanded={creating}>Create a workspace here</Button>
        </div>
      </div>

      {creating && <CreateWorkspace onCancel={() => setCreating(false)} />}

      {recentWorkspaces.length > 0 && <WorkspaceList label="Recent workspaces" title="Recent workspaces" items={recentWorkspaces} />}
      {found.length > 0 && <WorkspaceList label="Workspaces on this computer" title="On this computer" items={found} />}
    </div>
  );
}

/** Guided next steps for an empty workspace: import a spec, connect an agent. */
export function NextSteps({ specImported }: { specImported: boolean }) {
  const [importing, setImporting] = useState(!specImported);
  const [connected, setConnected] = useState(false);
  useEffect(() => {
    invoke("mcp_setup").then((s) => setConnected(s.claudeConnection === "connected"), () => {});
  }, []);
  return (
    <div className="steps">
      <h2 className="steps-title">Get started</h2>
      <ol className="steps-list">
        <li className={specImported ? "step is-done" : "step"}>
          <div className="step-head">
            <Icon name={specImported ? "check-circle" : "number-circle-one"} />
            <span className="step-name">Import an OpenAPI spec</span>
            {specImported && !importing && <Button size="sm" onClick={() => setImporting(true)}>Import another</Button>}
          </div>
          <p className="step-sub">Every operation becomes searchable, with the controller tag next to the path.</p>
          {importing && <SpecImportForm onDone={() => setImporting(false)} onCancel={specImported ? () => setImporting(false) : undefined} />}
        </li>
        <li className={connected ? "step is-done" : "step"}>
          <div className="step-head">
            <Icon name={connected ? "check-circle" : "number-circle-two"} />
            <span className="step-name">{connected ? "Agent connected" : "Connect an agent"}</span>
            <Button size="sm" icon="robot" onClick={() => openAgentSheet("connect")}>{connected ? "Agent setup" : "Open agent setup"}</Button>
          </div>
          <p className="step-sub">Let Claude Code or another MCP client search and send requests. You choose what it may send.</p>
        </li>
      </ol>
    </div>
  );
}
