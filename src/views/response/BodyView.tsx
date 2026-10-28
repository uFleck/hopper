import { useMemo, useRef, useState } from "react";
import type { RunResult } from "../../ipc";
import { Button, Segmented } from "../../ui";
import { CodeView, type CodeViewHandle } from "./CodeView";
import { CHUNK, LARGE_BODY, chunkEnd, formatBytes, headerValue, mimeType } from "./format";
import { prettyJson } from "../request/logic";

type Mode = "pretty" | "raw" | "preview";

function LargeBody({ text }: { text: string }) {
  const [end, setEnd] = useState(() => chunkEnd(text, 0));
  return (
    <div className="rv-large">
      <p className="rv-hint is-warn">Large response ({formatBytes(text.length)}): shown as plain text in chunks. Folding and search are off.</p>
      <pre className="rv-pre" tabIndex={0} aria-label="Response body">{text.slice(0, end)}</pre>
      {end < text.length && <Button size="sm" onClick={() => setEnd(chunkEnd(text, end, CHUNK))}>Show next {formatBytes(Math.min(CHUNK, text.length - end))}</Button>}
    </div>
  );
}

export interface BodyViewProps {
  run: RunResult;
  codeRef: React.Ref<CodeViewHandle>;
  mode: Mode;
  onMode: (m: Mode) => void;
}

export function BodyView({ run, codeRef, mode, onMode }: BodyViewProps) {
  const previewable = run.kind === "html" || run.kind === "image";
  const large = run.body.length > LARGE_BODY;
  const pretty = useMemo(() => (run.kind === "json" && !large ? (prettyJson(run.body) ?? run.body) : run.body), [run.body, run.kind, large]);
  const active: Mode = mode === "preview" && !previewable ? "pretty" : mode;
  const mime = mimeType(run.headers);
  const wrap = useRef(true);

  if (run.kind === "binary" || (run.kind === "image" && !run.bodyBase64 && active !== "preview" && !run.body)) {
    return (
      <div className="rv-placeholder">
        <p>Binary content, {formatBytes(run.decodedBytes)} ({mime}).</p>
        <p className="rv-hint">Not shown. Open it from the history or save it to a file once that is available.</p>
      </div>
    );
  }

  const hasText = run.body.length > 0 || run.kind !== "image";
  return (
    <div className="rv-bodyview">
      <div className="rv-bodybar">
        <Segmented size="sm" ariaLabel="Body view" value={active} onChange={onMode} options={[{ value: "pretty", label: "Pretty" }, { value: "raw", label: "Raw" }, { value: "preview", label: "Preview", disabled: !previewable }]} />
      </div>
      {active === "preview" && run.kind === "html" && <iframe className="rv-iframe" title="HTML preview" sandbox="" srcDoc={run.body} />}
      {active === "preview" && run.kind === "image" &&
        (run.bodyBase64 ? <div className="rv-image"><img alt="Response" src={`data:${mime};base64,${run.bodyBase64}`} /></div> : <p className="rv-hint">No image data.</p>)}
      {active !== "preview" && hasText && (run.body === "" ? <p className="rv-hint">The response has no body.</p> : large ? <LargeBody text={run.body} /> : <CodeView ref={codeRef} value={active === "raw" ? run.body : pretty} kind={active === "raw" ? "text" : run.kind} wrap={wrap.current} />)}
      {headerValue(run.headers, "content-encoding") && <p className="rv-hint">Content-Encoding: {headerValue(run.headers, "content-encoding")}, shown decoded.</p>}
    </div>
  );
}
