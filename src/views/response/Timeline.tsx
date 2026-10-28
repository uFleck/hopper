import type { RunResult } from "../../ipc";
import { Chip, statusTone } from "../../ui";
import { formatMs } from "./format";

export function Timeline({ run }: { run: RunResult }) {
  const { ttfbMs, downloadMs, totalMs } = run.timing;
  const total = Math.max(totalMs, 1);
  const bars: [string, number, string][] = [["Waiting (TTFB)", ttfbMs, "var(--i1)"], ["Download", downloadMs, "var(--i2)"]];
  return (
    <div className="rv-timeline">
      <h4 className="rv-sub">Timing</h4>
      {bars.map(([label, ms, color]) => (
        <div key={label} className="rv-bar">
          <span className="rv-bar-label">{label}</span>
          <span className="rv-bar-track"><span className="rv-bar-fill" style={{ width: `${Math.max(2, (ms / total) * 100)}%`, background: color }} /></span>
          <span className="rv-bar-ms">{formatMs(ms)}</span>
        </div>
      ))}
      <p className="rv-bar rv-bar--total"><span className="rv-bar-label">Total</span><span className="rv-bar-ms">{formatMs(totalMs)}</span></p>
      <p className="rv-hint">DNS, connect and TLS timings are not available yet. Only time to first byte, download and total are measured.</p>

      <h4 className="rv-sub">Redirects</h4>
      {run.redirects.length === 0 ? (
        <p className="rv-hint">No redirects.</p>
      ) : (
        <ol className="rv-hops">
          {run.redirects.map((h, i) => (
            <li key={i}><Chip tone={statusTone(h.status)}>{h.status}</Chip> <code>{h.url}</code></li>
          ))}
          <li><Chip tone={statusTone(run.status)}>{run.status}</Chip> <code>{run.finalUrl}</code> <span className="rv-hint">final</span></li>
        </ol>
      )}
      <p className="rv-hint">{run.httpVersion} · sent in {run.env}</p>
    </div>
  );
}
