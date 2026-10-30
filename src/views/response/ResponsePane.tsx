import { useEffect, useRef, useState, type CSSProperties } from "react";
import type { CookieDto, RunResult } from "../../ipc";
import { Button, Chip, Icon, IconButton, Select, Spinner, Tabs, confirm } from "../../ui";
import { cancel, openHistoryRun, toast, useStore, type RunState } from "../../store";
import { BodyView } from "./BodyView";
import type { CodeViewHandle } from "./CodeView";
import { clearCookies, listCookies } from "./cookies";
import { ErrorPanel } from "./ErrorPanel";
import { formatMs, sizeLabel, timeAgo } from "./format";
import { Timeline } from "./Timeline";

const statusColor = (s: number) => (s >= 500 || s === 0 ? "var(--s5)" : s >= 400 ? "var(--s4)" : s >= 300 ? "var(--s3)" : "var(--s2)");

/** The colour the circuit lights up in once a run has finished: green, blue, amber or red. Plain wire otherwise. */
export function runTone(run: RunState | null): string {
  if (run?.phase === "failed") return "var(--s5)";
  return run?.phase === "done" && run.result ? statusColor(run.result.status) : "var(--wire)";
}

type Tab = "body" | "headers" | "cookies" | "timeline";
const PHASE = { preparing: "Preparing the request", sending: "Waiting for the server", receiving: "Receiving the response" } as const;

function Elapsed({ since }: { since: number }) {
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    const t = setInterval(() => setNow(Date.now()), 100);
    return () => clearInterval(t);
  }, []);
  return <>{formatMs(now - since)}</>;
}

function Cookies() {
  const [rows, setRows] = useState<CookieDto[] | null>(null);
  const load = () => void listCookies().then(setRows, () => setRows([]));
  useEffect(load, []);
  const clear = async () => {
    if (!(await confirm({ title: "Clear all cookies?", message: "Every stored cookie for this workspace is removed.", tone: "danger", confirmLabel: "Clear" }))) return;
    await clearCookies();
    toast("Cookies cleared", "success");
    load();
  };
  if (!rows) return <p className="rv-hint">Loading</p>;
  return (
    <div>
      <div className="rv-row"><span className="rv-hint rv-grow">{rows.length} cookie{rows.length === 1 ? "" : "s"} in the jar</span><Button size="sm" icon="trash" disabled={!rows.length} onClick={() => void clear()}>Clear</Button></div>
      {rows.length > 0 && (
        <div className="rv-table-wrap">
          <table className="rv-table">
            <thead><tr><th>Name</th><th>Value</th><th>Domain</th><th>Flags</th></tr></thead>
            <tbody>
              {rows.map((c) => (
                <tr key={`${c.domain}${c.path}${c.name}`}>
                  <td>{c.name}</td>
                  <td data-tip={c.value}>{c.value.length > 24 ? `${c.value.slice(0, 24)}…` : c.value}</td>
                  <td>{c.domain}{c.path}</td>
                  <td>{[c.secure && "secure", c.httpOnly && "httpOnly"].filter(Boolean).join(", ")}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}

function Result({ run, requestId, maximized, onMaximize }: { run: RunResult; requestId: string | null; maximized: boolean; onMaximize: () => void }) {
  const [tab, setTab] = useState<Tab>("body");
  const [mode, setMode] = useState<"pretty" | "raw" | "preview">(run.kind === "html" || run.kind === "image" ? "preview" : "pretty");
  const code = useRef<CodeViewHandle>(null);
  const history = useStore((s) => (requestId ? s.history[requestId] : undefined));

  useEffect(() => setMode(run.kind === "html" || run.kind === "image" ? "preview" : "pretty"), [run.runId, run.kind]);

  const copyBody = async () => {
    try {
      await navigator.clipboard.writeText(run.body);
      toast("Copied the response body", "success");
    } catch {
      toast("The clipboard is not available.", "error");
    }
  };

  const copyBodyPath = async () => {
    try {
      await navigator.clipboard.writeText(run.bodyFile ?? "");
      toast("Copied the file path", "success");
    } catch {
      toast("The clipboard is not available.", "error");
    }
  };

  return (
    <>
      <div className="rv-summary">
        <div className="rv-bigst" style={{ "--c": statusColor(run.status) } as CSSProperties}>
          <b>{run.status}</b>
          <span>{run.statusText}</span>
        </div>
        <p className="rv-rmeta" data-tip={`Waiting ${formatMs(run.timing.ttfbMs)}, download ${formatMs(run.timing.downloadMs)}`}>
          {formatMs(run.timing.totalMs)} <em>= {Math.max(1, Math.round(run.timing.totalMs / 50))} tick{run.timing.totalMs >= 75 ? "s" : ""}</em> · {sizeLabel(run.decodedBytes, run.transferredBytes)} <em>{Math.max(1, Math.ceil(run.decodedBytes / 64))} stack{run.decodedBytes > 64 ? "s" : ""}</em>
        </p>
        {run.insecureTls && <Chip tone="err" tip="This response came over a connection that did not verify the certificate">TLS verification off</Chip>}
        {run.redirects.length > 0 && <button type="button" className="rv-linkchip" onClick={() => setTab("timeline")}>{run.redirects.length} redirect{run.redirects.length === 1 ? "" : "s"}</button>}
        <span className="rv-grow" />
        {history && history.length > 0 && requestId && (
          <Select aria-label="Run history" className="rv-hist" value={history.some((h) => h.id === run.historyId) ? run.historyId : ""} onChange={(e) => e.target.value && void openHistoryRun(requestId, e.target.value)}>
            {!history.some((h) => h.id === run.historyId) && <option value="">This run</option>}
            {history.map((h) => (
              <option key={h.id} value={h.id}>{`${h.status} · ${formatMs(h.totalMs)} · ${timeAgo(h.sentAt)} · ${h.env}`}</option>
            ))}
          </Select>
        )}
        <IconButton icon="magnifying-glass" label="Search in body (Ctrl+F)" size="sm" disabled={tab !== "body"} onClick={() => code.current?.search()} />
        <IconButton icon="copy" label="Copy body" size="sm" onClick={() => void copyBody()} />
        <IconButton icon={maximized ? "arrows-in" : "arrows-out"} label={maximized ? "Restore" : "Maximize response"} size="sm" active={maximized} onClick={onMaximize} />
      </div>
      {run.truncated && <p className="rv-hint is-warn" role="status"><Icon name="scissors" /> The body was cut at the size limit; what you see is the start of it.</p>}
      {run.bodyFile && (
        <div className="rv-row">
          <p className="rv-hint rv-grow" role="status" data-tip={run.bodyFile}><Icon name="file" /> Large response: the full body is saved to a file, and your agent can read it too.</p>
          <Button size="sm" icon="copy" onClick={() => void copyBodyPath()}>Copy path</Button>
        </div>
      )}

      <Tabs ariaLabel="Response" value={tab} onChange={setTab} tabs={[{ value: "body", label: "Body" }, { value: "headers", label: "Headers", badge: run.headers.length }, { value: "cookies", label: "Cookies" }, { value: "timeline", label: "Timeline" }]} />
      <div className="rv-tabpanel" role="tabpanel">
        {tab === "body" && <BodyView run={run} codeRef={code} mode={mode} onMode={setMode} />}
        {tab === "headers" && (
          <div className="rv-table-wrap">
            <table className="rv-table">
              <tbody>{run.headers.map(([k, v], i) => <tr key={i}><th scope="row">{k}</th><td>{v}</td></tr>)}</tbody>
            </table>
          </div>
        )}
        {tab === "cookies" && <Cookies />}
        {tab === "timeline" && <Timeline run={run} />}
      </div>
    </>
  );
}

export interface ResponsePaneProps {
  run: RunState | null;
  requestId: string | null;
  onRetry: () => void;
}

export function ResponsePane({ run, requestId, onRetry }: ResponsePaneProps) {
  const [maximized, setMaximized] = useState(false);
  const inFlight = run && (run.phase === "preparing" || run.phase === "sending" || run.phase === "receiving");
  return (
    <section
      className={`rv-resp${maximized ? " rv-resp--max" : ""}`}
      aria-label="Response"
      onKeyDown={(e) => {
        if (e.key === "Escape" && maximized) {
          e.preventDefault();
          e.stopPropagation();
          setMaximized(false);
        }
      }}
    >
      {!run && <div className="rv-empty"><Icon name="paper-plane-tilt" /><b>No response yet</b><span>Send the request and the wire lights up. <kbd className="ui-kbd">Ctrl</kbd> <kbd className="ui-kbd">Enter</kbd></span></div>}
      {inFlight && (
        <div className="rv-flight" role="status">
          <Spinner size={18} />
          <div><strong>{PHASE[run.phase as keyof typeof PHASE]}</strong><div className="rv-hint"><Elapsed since={run.startedAt} /></div></div>
          <Button icon="x" onClick={() => void cancel(run.runId)}>Cancel</Button>
        </div>
      )}
      {run?.phase === "cancelled" && <p className="rv-empty"><Icon name="prohibit" /> Cancelled. Nothing was kept.</p>}
      {run?.phase === "failed" && run.error && <ErrorPanel error={run.error} onRetry={onRetry} />}
      {run?.phase === "done" && run.result && <Result run={run.result} requestId={requestId} maximized={maximized} onMaximize={() => setMaximized((m) => !m)} />}
    </section>
  );
}
