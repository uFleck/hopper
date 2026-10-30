import { useMemo } from "react";
import type { Body, KeyValue, MultipartPart } from "../../ipc";
import { Button, Editor, IconButton, Select, type VariableOption } from "../../ui";
import { toast } from "../../store";
import { BODY_TYPES, jsonStatus, prettyJson, switchBody, type BodyType, type VarInfo } from "./logic";
import { KeyValueTable } from "./KeyValueTable";
import { VarInput } from "./VarInput";
import { VarPopover } from "./VarPopover";

const LABEL: Record<BodyType, string> = { none: "None", json: "JSON", text: "Text", xml: "XML", form: "Form (urlencoded)", multipart: "Multipart", binary: "Binary file" };

export interface BodySectionProps {
  body: Body;
  onChange: (body: Body) => void;
  info: VarInfo;
  variables: VariableOption[];
  readOnly: boolean;
  onSave: () => void;
}

export function BodySection({ body, onChange, info, variables, readOnly, onSave }: BodySectionProps) {
  const status = useMemo(() => (body.type === "json" ? jsonStatus(body.content) : null), [body]);
  const pretty = () => {
    if (body.type !== "json") return;
    const out = prettyJson(body.content);
    if (out === null) toast("The body is not valid JSON, so it can't be pretty-printed.", "error");
    else onChange({ type: "json", content: out });
  };

  return (
    <div className="rv-body-sec">
      <div className="rv-row">
        <Select aria-label="Body type" value={body.type} disabled={readOnly} onChange={(e) => onChange(switchBody(body, e.target.value as BodyType))} className="rv-grow">
          {BODY_TYPES.map((t) => (
            <option key={t} value={t}>{LABEL[t]}</option>
          ))}
        </Select>
        {body.type === "json" && (
          <Button size="sm" icon="text-align-left" onClick={pretty} disabled={readOnly || status?.state !== "valid"} data-tip="Pretty-print the JSON">Pretty-print</Button>
        )}
      </div>

      {(body.type === "json" || body.type === "text" || body.type === "xml") && (
        <>
          <Editor
            value={body.content}
            onChange={(content) => onChange({ type: body.type, content } as Body)}
            language={body.type === "text" ? "text" : body.type}
            readOnly={readOnly}
            known={info.known}
            variables={variables}
            renderVariableTip={(name, { rename }) => <VarPopover name={name} info={info} rename={rename} />}
            onSave={onSave}
            ariaLabel={`${LABEL[body.type]} body`}
            minHeight={110}
            maxHeight={320}
            placeholder={body.type === "json" ? '{ "key": "{{variable}}" }' : undefined}
          />
          {status && status.state !== "empty" && (
            <p className={`rv-hint ${status.state === "invalid" ? "is-err" : "is-ok"}`} role="status">
              {status.state === "valid" ? `Valid JSON${status.note ? `, ${status.note}` : ""}` : `Invalid JSON: ${status.message}`}
            </p>
          )}
        </>
      )}

      {body.type === "form" && <KeyValueTable rows={body.fields} info={info} noun="form field" disabled={readOnly} onChange={(fields: KeyValue[]) => onChange({ type: "form", fields })} />}

      {body.type === "multipart" && <MultipartTable parts={body.parts} info={info} disabled={readOnly} onChange={(parts) => onChange({ type: "multipart", parts })} />}

      {body.type === "binary" && (
        <>
          <VarInput value={body.file} info={info} ariaLabel="Binary file path" placeholder="C:\path\to\file.bin" disabled={readOnly} onChange={(file) => onChange({ type: "binary", file })} />
          <p className="rv-hint">The file is read when you send. Type or paste its path.</p>
        </>
      )}

      {body.type === "none" && <p className="rv-hint">This request sends no body.</p>}
    </div>
  );
}

const isFile = (p: MultipartPart): p is Extract<MultipartPart, { file: string }> => "file" in p;

function MultipartTable({ parts, onChange, info, disabled }: { parts: MultipartPart[]; onChange: (p: MultipartPart[]) => void; info: VarInfo; disabled: boolean }) {
  const shown: MultipartPart[] = disabled ? parts : [...parts, { name: "", enabled: true, value: "" }];
  const set = (i: number, next: MultipartPart) => onChange(i >= parts.length ? [...parts, next] : parts.map((p, j) => (j === i ? next : p)));
  return (
    <div className="rv-kv" role="group" aria-label="multipart parts">
      {shown.map((p, i) => {
        const ghost = i >= parts.length;
        return (
          <div key={i} className={`rv-kv-row rv-kv-row--part${ghost ? " is-ghost" : ""}${p.enabled ? "" : " is-off"}`}>
            <input type="checkbox" className="rv-check" checked={p.enabled} disabled={disabled || ghost} aria-label={`Enable part ${p.name || i + 1}`} onChange={(e) => set(i, { ...p, enabled: e.target.checked })} />
            <VarInput value={p.name} info={info} ariaLabel={`Part ${i + 1} name`} placeholder="name" disabled={disabled} onChange={(name) => set(i, { ...p, name })} />
            <Select aria-label={`Part ${i + 1} kind`} value={isFile(p) ? "file" : "text"} disabled={disabled} onChange={(e) => set(i, e.target.value === "file" ? { name: p.name, enabled: p.enabled, file: "" } : { name: p.name, enabled: p.enabled, value: "" })} className="rv-kind">
              <option value="text">Text</option>
              <option value="file">File</option>
            </Select>
            <VarInput value={isFile(p) ? p.file : p.value} info={info} ariaLabel={`Part ${i + 1} ${isFile(p) ? "file path" : "value"}`} placeholder={isFile(p) ? "C:\\path\\to\\file" : "value"} disabled={disabled} onChange={(v) => set(i, isFile(p) ? { ...p, file: v } : { ...p, value: v })} />
            {ghost || disabled ? <span className="rv-kv-gap" /> : <IconButton icon="x" size="sm" label={`Remove part ${p.name || i + 1}`} onClick={() => onChange(parts.filter((_, j) => j !== i))} />}
          </div>
        );
      })}
    </div>
  );
}
