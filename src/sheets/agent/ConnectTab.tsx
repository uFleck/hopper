import { useEffect, useState } from "react";
import { toHopperError, type McpAgentSetup, type McpSetup } from "../../ipc";
import { refreshMcp, toast, useStore } from "../../store";
import { Busy, Button, Chip } from "../../ui";
import { connectAgent, getMcpSetup } from "../env/api";
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

interface RowOutput {
  text: string;
  ok: boolean;
}

/** One agent: its state, a Connect button, and the manual command/JSON for when its CLI is missing. */
function AgentRow({ agent, disabled, busy, output, onConnect }: {
  agent: McpAgentSetup;
  disabled: boolean;
  busy: boolean;
  output: RowOutput | null;
  onConnect: () => void;
}) {
  return (
    <section className="ag-agent">
      <div className="ag-agent-head">
        <span className="ag-agent-name">{agent.name}</span>
        {agent.connection === "connected" && <Chip tone="ok">connected</Chip>}
        {agent.connection === "stale" && <Chip tone="warn">outdated</Chip>}
        {agent.connection === "none" && <Chip>not connected</Chip>}
        {!agent.detected && <span className="ag-agent-missing">CLI not found</span>}
      </div>
      {agent.connection === "connected" && <Note>Connected to this Hopper. Reconnect only if something stops working.</Note>}
      {agent.connection === "stale" && <Note tone="warn">The Hopper entry in {agent.name} is outdated (the port or token changed). Reconnect to update it.</Note>}
      {!agent.detected && agent.connection === "none" && (
        <Note tone="warn">Hopper could not find {agent.name} on this machine. Install it, then reconnect, or copy the JSON below into its config.</Note>
      )}
      <div className="sh-row-actions">
        <Button
          variant={agent.connection === "connected" ? "secondary" : "primary"}
          icon="plug"
          loading={busy}
          disabled={disabled}
          onClick={onConnect}
        >
          {agent.connection === "connected" ? `Reconnect ${agent.name}` : `Connect ${agent.name}`}
        </Button>
      </div>
      {output && <pre className={output.ok ? "ag-pre" : "ag-pre is-error"} role="status">{output.text}</pre>}
      <details className="ag-manual">
        <summary className="ui-field-label">Manual setup</summary>
        <CopyBlock label={`${agent.name} command`} text={agent.command} />
        <CopyBlock label={`${agent.name} JSON config`} text={agent.jsonConfig} />
      </details>
    </section>
  );
}

export function ConnectTab() {
  const mcp = useStore((s) => s.mcp);
  const [setup, setSetup] = useState<McpSetup | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [output, setOutput] = useState<(RowOutput & { id: string }) | null>(null);

  const load = () => getMcpSetup().then(setSetup, (e) => toast(toHopperError(e).message, "error"));

  useEffect(() => {
    void refreshMcp();
    void load();
  }, [mcp?.running]);

  const connect = async (agent: McpAgentSetup) => {
    setBusy(agent.id);
    setOutput(null);
    try {
      setOutput({ id: agent.id, text: await connectAgent(agent.id), ok: true });
      await load();
    } catch (e) {
      setOutput({ id: agent.id, text: toHopperError(e).message, ok: false });
    } finally {
      setBusy(null);
    }
  };

  return (
    <div className="ag-connect">
      <div className="ag-status">
        <Chip tone={mcp?.running ? "ok" : "err"}>{mcp?.running ? "running" : "not running"}</Chip>
        {mcp?.port != null && <span>port <code>{mcp.port}</code></span>}
        {mcp?.url && <code className="ag-url">{mcp.url}</code>}
      </div>
      {mcp?.error && <Note tone="danger">{mcp.error}</Note>}
      <Note tone="warn">The token in these commands is a local credential for this machine. Anyone who has it can use Hopper's agent tools, so do not paste it into chats, tickets or git.</Note>
      <Note>Connecting installs Hopper for every project on this machine (each agent's user scope).</Note>
      {!setup ? (mcp?.running ? <Busy /> : <Note tone="warn">The MCP server is not running, so there is nothing to connect to yet.</Note>) : setup.agents.map((agent) => (
        <AgentRow
          key={agent.id}
          agent={agent}
          disabled={!mcp?.running || busy !== null}
          busy={busy === agent.id}
          output={output?.id === agent.id ? output : null}
          onConnect={() => void connect(agent)}
        />
      ))}
    </div>
  );
}
