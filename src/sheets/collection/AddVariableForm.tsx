import { useState } from "react";
import type { Stage } from "../../ipc";
import { toast, useStore } from "../../store";
import { Button, Icon, TextInput } from "../../ui";
import { useCollectionInfo } from "../../views/request/useCollectionInfo";
import { VarInput } from "../../views/request/VarInput";
import { Segmented, Switch, attempt } from "./controls";
import { LOOKS_SECRET, STAGES, STAGE_LABEL, nameProblem, type MatrixRow } from "./matrix";
import { addSecret, addVariable } from "./ops";

type Mode = "same" | "each";
type PerStage = Partial<Record<Stage, string>>;

/** One form for every kind of variable: "Secret" is a switch, and a value is either the same everywhere or per environment. */
export function AddVariableForm({ slug, rows, onDone, onCancel }: { slug: string; rows: MatrixRow[]; onDone: () => void; onCancel: () => void }) {
  const stages = useStore((s) => s.environments);
  const active = stages.find((s) => s.active)?.stage ?? null;
  const writable = STAGES.filter((s) => !stages.find((x) => x.stage === s)?.readOnly);
  const [name, setName] = useState("");
  const [secretChoice, setSecretChoice] = useState<boolean | null>(null);
  const [mode, setMode] = useState<Mode>("same");
  const [value, setValue] = useState("");
  const [per, setPer] = useState<PerStage>({});
  const [busy, setBusy] = useState(false);
  const info = useCollectionInfo(slug, true);

  const trimmed = name.trim();
  const secret = secretChoice ?? LOOKS_SECRET.test(trimmed);
  const problem = nameProblem(trimmed, rows);
  const filled = STAGES.some((s) => per[s]);
  const ready = !!trimmed && !problem && (secret || (mode === "same" ? value !== "" : filled));

  const submit = async () => {
    if (!ready) return;
    setBusy(true);
    const ok = await attempt(() => (secret ? addSecret(slug, trimmed, per) : addVariable(slug, trimmed, mode === "same" ? value : per)));
    setBusy(false);
    if (!ok) return;
    toast(`Added {{${trimmed}}}`, "success");
    onDone();
  };

  const stageInput = (s: Stage, isSecret: boolean) => (
    <div className="cv-envrow" key={s}>
      <span className={s === active ? "is-active" : undefined}>{STAGE_LABEL[s]}</span>
      <TextInput
        mono
        type={isSecret ? "password" : "text"}
        autoComplete={isSecret ? "new-password" : undefined}
        aria-label={`${trimmed || "Variable"} in ${STAGE_LABEL[s]}`}
        placeholder={isSecret ? (s === active ? "Paste the value" : "Skip for now") : "Skip to use none"}
        disabled={!writable.includes(s)}
        value={per[s] ?? ""}
        onChange={(e) => setPer({ ...per, [s]: e.target.value })}
      />
    </div>
  );

  return (
    <form className="cv-form" onSubmit={(e) => (e.preventDefault(), void submit())} onKeyDown={(e) => e.key === "Escape" && (e.preventDefault(), e.stopPropagation(), onCancel())}>
      <div className="cv-formhead"><b>New variable</b></div>
      <label className="cv-field">
        <span className="cv-lbl">Name</span>
        <TextInput mono autoFocus placeholder="baseUrl" value={name} invalid={!!problem} onChange={(e) => setName(e.target.value)} />
        {problem ? <span className="cv-bad" role="alert">{problem}</span> : trimmed ? <span className="cv-good"><Icon name="check" /> Free in this collection</span> : null}
      </label>

      <Switch checked={secret} onChange={setSecretChoice} label="Secret" hint={secretChoice === null && secret ? "switched on: the name looks like a credential" : "keep the value out of git"} />

      {secret ? (
        <div className="cv-field">
          <span className="cv-lbl">Value per environment</span>
          <div className="cv-envs">{STAGES.map((s) => stageInput(s, true))}</div>
          <p className="cv-note"><Icon name="lock-simple" /> Stored in the OS credential store on this machine. Never written to git, and never shown again.</p>
        </div>
      ) : (
        <>
          <div className="cv-field">
            <span className="cv-lbl">Applies to</span>
            <Segmented<Mode> label="Applies to" value={mode} onChange={setMode} options={[{ value: "same", label: "Same everywhere" }, { value: "each", label: "Differs per environment" }]} />
          </div>
          {mode === "same" ? (
            <label className="cv-field">
              <span className="cv-lbl">Value</span>
              <VarInput info={info} canDefine={false} ariaLabel="Value" placeholder="value, or {{another}}" value={value} onChange={setValue} />
            </label>
          ) : (
            <div className="cv-field">
              <span className="cv-lbl">Value per environment</span>
              <div className="cv-envs">{STAGES.map((s) => stageInput(s, false))}</div>
            </div>
          )}
        </>
      )}

      <div className="cv-actions">
        <Button variant="ghost" onClick={onCancel}>Cancel</Button>
        <Button type="submit" variant="primary" loading={busy} disabled={!ready}>{secret ? "Save secret" : "Add variable"}</Button>
      </div>
    </form>
  );
}
