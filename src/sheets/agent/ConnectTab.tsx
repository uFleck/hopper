import { useEffect, useState } from "react";
import { toHopperError, type McpAgentSetup, type McpSetup } from "../../ipc";
import { refreshMcp, toast, useStore } from "../../store";
import { Busy, Button, Chip } from "../../ui";
import { connectAgent, getMcpSetup } from "../env/api";
import { Note, copyText } from "../env/controls";

function CopyBlock({ label, text, wrap = false }: { label: string; text: string; wrap?: boolean }) {
  const copy = async () => toast((await copyText(text)) ? `${label} copied` : "Copy failed: select the text and copy it by hand", "info");
  return (
    <div className="ag-copy">
      <div className="ag-copy-head"><span className="ui-field-label">{label}</span><Button size="sm" icon="copy" onClick={() => void copy()} aria-label={`Copy ${label}`}>Copy</Button></div>
      <pre className={wrap ? "ag-pre is-wrap" : "ag-pre"} tabIndex={0}>{text}</pre>
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

/**
 * Executor registers MCP servers through its own AI, not through a CLI or a config file Hopper can
 * write. So this row is instructions only: detection on PATH, manual import steps and a copyable
 * prompt carrying just the local URL. No token is shown for this target.
 */
function ExecutorRow({ agent }: { agent: McpAgentSetup }) {
  return (
    <section className="ag-agent">
      <div className="ag-agent-head">
        <span className="ag-agent-name">{agent.name}</span>
        <Chip tone={agent.detected ? "ok" : "warn"}>{agent.detected ? "found on this machine" : "not found on PATH"}</Chip>
      </div>
      <Note>Executor adds MCP servers through its own AI. Copy the prompt below and paste it into Executor to add Hopper. It contains only this machine's local URL — no token or secret.</Note>
      <CopyBlock label="Prompt for Executor" text={agent.aiPrompt} wrap />
      <details className="ag-manual">
        <summary className="ui-field-label">Manual steps</summary>
        <ol className="ag-steps">
          <li>{agent.detected ? "Open Executor on this machine." : "Install Executor on this machine, then open it."}</li>
          <li>Start a chat with Executor's AI and paste the prompt above.</li>
          <li>If Executor asks for approval to add the app, review and approve it.</li>
        </ol>
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
        agent.id === "executor" ? (
          <ExecutorRow key={agent.id} agent={agent} />
        ) : (
          <AgentRow
            key={agent.id}
            agent={agent}
            disabled={!mcp?.running || busy !== null}
            busy={busy === agent.id}
            output={output?.id === agent.id ? output : null}
            onConnect={() => void connect(agent)}
          />
        )
      ))}
    </div>
  );
}
