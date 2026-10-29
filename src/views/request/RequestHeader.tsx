import type { Method } from "../../ipc";
import { Button, Chip, Editor, Icon, IconButton, Select, cx, type VariableOption } from "../../ui";
import { copyCurl, deleteRequest, duplicateRequest, editDraft, toast, togglePin, usePins, type DraftState, type RunState } from "../../store";
import { AgentChip } from "./AgentChip";
import { resolveUrl, type ResolvedEnv, type VarInfo } from "./logic";
import { Menu } from "./Menu";
import { VarPopover } from "./VarPopover";

const METHODS: Method[] = ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];

async function copyCurlText(masked: boolean) {
  const text = await copyCurl(masked);
  if (!text) return;
  try {
    await navigator.clipboard.writeText(text);
    toast(masked ? "Copied cURL, secrets masked" : "Copied cURL with real secret values", masked ? "success" : "info");
  } catch {
    toast("The clipboard is not available.", "error");
  }
}

export interface RequestHeaderProps {
  draft: DraftState;
  env: ResolvedEnv | null;
  info: VarInfo;
  variables: VariableOption[];
  run: RunState | null;
  /** Production writes: the arm switch must be on before Send works. */
  needArm: boolean;
  armed: boolean;
  onArm: () => void;
  onSend: () => void;
  onCancel: () => void;
  onSave: () => void;
  onBack: () => void;
}

/** Back, name, pin, actions, then the URL bar with Send inside it, and the URL as it will be sent. */
export function RequestHeader({ draft, env, info, variables, run, needArm, armed, onArm, onSend, onCancel, onSave, onBack }: RequestHeaderProps) {
  const { doc } = draft;
  const readOnly = draft.base.readOnly;
  const saved = draft.base.revision !== null && !!doc.id;
  const inFlight = run && (run.phase === "preparing" || run.phase === "sending" || run.phase === "receiving");
  const resolved = resolveUrl(doc, info);
  const resolvedText = resolved.segments.map((s) => s.text).join("");
  const insecure = !!env?.tls.insecureSkipVerify;
  const pinned = usePins().includes(doc.id);

  return (
    <div className="rv-head">
      <div className="rv-row rv-title">
        <IconButton icon="caret-left" label="Back" size="sm" onClick={onBack} />
        <input className="rv-name" aria-label="Request name" value={doc.name} readOnly={readOnly} placeholder="Request name" spellCheck={false} onChange={(e) => editDraft({ name: e.target.value })} />
        <AgentChip requestId={saved ? doc.id : null} access={draft.base.agentAccess} suspended={draft.base.grantSuspended} />
        <IconButton icon="star" label={pinned ? "Unpin from home" : "Pin to home"} size="sm" active={pinned} className={cx("rv-pin", pinned && "is-pinned")} disabled={!saved} onClick={() => togglePin(doc.id)} />
        {draft.dirty && (
          <Button size="sm" icon="floppy-disk" loading={draft.saving} disabled={readOnly} onClick={onSave} title={saved ? "Unsaved changes. Save (Ctrl+S)" : "Not saved yet. Save (Ctrl+S)"}>Save</Button>
        )}
        <Menu
          ariaLabel="Request actions"
          triggerClassName="ui-iconbtn ui-iconbtn--sm"
          trigger={<Icon name="dots-three-vertical" />}
          items={[
            { label: "Save", hint: "Ctrl+S", icon: "floppy-disk", disabled: readOnly || (!draft.dirty && saved), onSelect: onSave },
            { label: "Duplicate", icon: "copy", disabled: !saved, onSelect: () => void duplicateRequest(doc.id) },
            { label: "Copy cURL, masked", hint: "Safe to paste into chat", icon: "terminal", onSelect: () => void copyCurlText(true) },
            { label: "Copy cURL, resolved", hint: "Includes real secret values", icon: "warning", tone: "warn", title: "Contains real secret values. Do not paste it into chat or commit it.", onSelect: () => void copyCurlText(false) },
            { label: "Delete", icon: "trash", tone: "danger", disabled: !saved, onSelect: () => void deleteRequest(doc.id) },
          ]}
        />
      </div>

      <div className="rv-urlrow">
        <Select aria-label="Method" className={`rv-method tone-${doc.method.toLowerCase()}`} value={doc.method} disabled={readOnly} onChange={(e) => editDraft({ method: e.target.value as Method })}>
          {METHODS.map((m) => (
            <option key={m} value={m}>{m}</option>
          ))}
        </Select>
        <Editor
          className="rv-url"
          value={doc.url}
          onChange={(v) => editDraft({ url: v.replace(/[\r\n]/g, "") })}
          readOnly={readOnly}
          known={info.known}
          variables={variables}
          renderVariableTip={(name, { rename }) => <VarPopover name={name} info={info} rename={rename} />}
          onSave={onSave}
          lineNumbers={false}
          wrap={false}
          minHeight={0}
          ariaLabel="URL"
          placeholder="{{baseUrl}}/path/{id}"
        />
        {needArm && (
          <button type="button" className={cx("rv-arm", armed && "is-on")} role="switch" aria-checked={armed} onClick={onArm} title="Arm for 5 seconds, then Send">
            <span className="rv-arm-sw" aria-hidden="true" />
            Arm
          </button>
        )}
        {inFlight && <button type="button" className="rv-cancel" onClick={onCancel}>Cancel</button>}
        <button type="button" className={cx("rv-send", needArm && armed && "is-red", inFlight && "is-busy")} disabled={!!inFlight || (needArm && !armed)} onClick={onSend} title="Send (Ctrl+Enter)">
          <span className="rv-drip" aria-hidden="true" />
          <span>{inFlight ? "Sending…" : "Send"}</span>
          {!inFlight && <kbd className="ui-kbd">↵</kbd>}
        </button>
      </div>

      {resolvedText !== doc.url && (
        <p className="rv-resolved" aria-label="Resolved URL" title={resolvedText}>
          <code>
            {resolved.segments.map((s, i) => (
              <span key={i} className={`rv-seg rv-seg--${s.kind}`} title={s.kind === "missing" ? "Secret has no value" : undefined}>{s.text}</span>
            ))}
          </code>
        </p>
      )}
      {resolved.emptyPathParams.length > 0 && (
        <p className="rv-warnline" role="status">
          <Icon name="warning" />
          <span>Path value missing: {resolved.emptyPathParams.map((n) => `{${n}}`).join(", ")}.</span>
        </p>
      )}
      {insecure && <Chip tone="err" title="The active environment does not verify TLS certificates">TLS verification off</Chip>}
    </div>
  );
}
