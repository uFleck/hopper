import { useEffect, useState } from "react";
import { toHopperError, type McpSetup } from "../../ipc";
import { refreshMcp, toast, useStore } from "../../store";
import { Busy, Button, Chip } from "../../ui";
import { connectClaudeCode, getMcpSetup } from "../env/api";
import { Note, copyText } from "../env/controls";

function CopyBlock({ label, text }: { label: string; text: string }) {
  const copy = async () => toast((await copyText(text)) ? `${label} copied` : "Copy failed: select the text and copy it by hand", "info");
  return (
    <div className="ag-copy">
      <div className="ag-copy-head"><span className="ui-field-label">{label}</span><Button size="sm" icon="copy" onClick={() => void copy()} aria-label={`Copy ${label}`}>Copy</Button></div>
      <pre className="ag-pre" tabIndex={0}>{text}</pre>
    </div>
  );
}

export function ConnectTab() {
  const mcp = useStore((s) => s.mcp);
  const [setup, setSetup] = useState<McpSetup | null>(null);
  const [output, setOutput] = useState<{ text: string; ok: boolean } | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    void refreshMcp();
    getMcpSetup().then(setSetup, (e) => toast(toHopperError(e).message, "error"));
  }, []);

  const connect = async () => {
    setBusy(true);
    try {
      setOutput({ text: await connectClaudeCode(), ok: true });
      getMcpSetup().then(setSetup, () => {});
    } catch (e) {
      setOutput({ text: toHopperError(e).message, ok: false });
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="ag-connect">
      <div className="ag-status">
        <Chip tone={mcp?.running ? "ok" : "err"}>{mcp?.running ? "running" : "not running"}</Chip>
        {setup?.claudeConnection === "connected" && <Chip tone="ok">Claude Code connected</Chip>}
        {mcp?.port != null && <span>port <code>{mcp.port}</code></span>}
        {mcp?.url && <code className="ag-url">{mcp.url}</code>}
      </div>
      {mcp?.error && <Note tone="danger">{mcp.error}</Note>}
      {setup?.claudeConnection === "connected" && <Note>Claude Code is connected to this Hopper. Reconnect only if something stops working.</Note>}
      {setup?.claudeConnection === "stale" && <Note tone="warn">Claude Code has an older Hopper entry (the port or token changed). Reconnect to update it.</Note>}
      <Note tone="warn">The token in these commands is a local credential for this machine. Anyone who has it can use Hopper's agent tools, so do not paste it into chats, tickets or git.</Note>
      <Note>Connecting installs Hopper for every project on this machine (Claude Code user scope).</Note>
      <div className="sh-row-actions">
        <Button variant="primary" icon="plug" loading={busy} disabled={!mcp?.running} onClick={() => void connect()}>{setup?.claudeConnection === "none" || !setup ? "Connect Claude Code" : "Reconnect Claude Code"}</Button>
      </div>
      {output && <pre className={output.ok ? "ag-pre" : "ag-pre is-error"} role="status">{output.text}</pre>}
      {setup ? (
        <>
          <CopyBlock label="Claude Code command" text={setup.claudeCommand} />
          <CopyBlock label="JSON config" text={setup.jsonConfig} />
        </>
      ) : <Busy />}
    </div>
  );
}
