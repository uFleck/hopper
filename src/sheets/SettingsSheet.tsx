import { useEffect, useRef, useState } from "react";
import type { Settings } from "../ipc";
import { closeSheet, closeWorkspace, updateSettings, useStore } from "../store";
import { Button, Field, Kbd, Segmented, Sheet, confirm } from "../ui";
import { Section, Switch } from "./env/controls";
import { RECORD_HINTS, comboFromEvent, comboParts } from "./hotkey";
import UpdatesSection from "./UpdatesSection";

function HotkeyField({ settings }: { settings: Settings }) {
  const [recording, setRecording] = useState(false);
  const [hint, setHint] = useState<string | null>(null);
  const btn = useRef<HTMLButtonElement>(null);
  // TODO(contract): `Settings` has no registration status. When the backend adds one (e.g. `hotkeyError`), it shows here.
  const failure = (settings as Settings & { hotkeyError?: string | null }).hotkeyError;

  useEffect(() => {
    if (!recording) return;
    const onKey = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (e.key === "Escape") {
        setRecording(false);
        setHint(null);
        return;
      }
      const r = comboFromEvent(e);
      if (!r.ok) return setHint(RECORD_HINTS[r.reason]);
      setRecording(false);
      setHint(null);
      void updateSettings({ hotkey: r.combo });
      btn.current?.focus();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [recording]);

  return (
    <Field label="Summon hotkey" hint={recording ? (hint ?? "Press the combination you want. Esc cancels.") : "Opens the panel from anywhere. The tray icon always works."} error={failure ? `Could not register this hotkey: ${failure}` : null}>
      {() => (
        <div className="st-hotkey">
          <span className="st-keys" aria-label={`Current hotkey ${settings.hotkey}`}>
            {recording ? <em>Listening…</em> : comboParts(settings.hotkey).map((p) => <Kbd key={p}>{p}</Kbd>)}
          </span>
          <Button ref={btn} size="sm" onClick={() => { setHint(null); setRecording((v) => !v); }} aria-pressed={recording} onBlur={() => setRecording(false)}>{recording ? "Cancel" : "Change"}</Button>
        </div>
      )}
    </Field>
  );
}

export default function SettingsSheet() {
  const settings = useStore((s) => s.settings);
  const workspace = useStore((s) => s.workspace);

  const closeWs = async () => {
    if (await confirm({ title: "Close this workspace?", message: "Your files stay where they are. You can reopen it from the start screen.", confirmLabel: "Close workspace", tone: "danger" })) await closeWorkspace();
  };

  return (
    <Sheet open title="Settings" onClose={closeSheet}>
      {!settings ? <p className="sh-empty">Loading settings…</p> : (
        <div className="st">
          <Section title="Panel">
            <HotkeyField settings={settings} />
            <Field label="Theme">
              {() => <Segmented ariaLabel="Theme" value={settings.theme} options={[{ value: "dark", label: "Dark" }, { value: "light", label: "Light" }, { value: "system", label: "System" }]} onChange={(theme) => void updateSettings({ theme })} />}
            </Field>
            <Switch checked={settings.alwaysShowTab} onChange={(alwaysShowTab) => void updateSettings({ alwaysShowTab })} label="Always show the edge handle" hint="Keep Hopper on the screen edge when the panel is closed. Drag it to the other edge; click it to open the panel." />
            <Switch checked={settings.startWithWindows === true} disabled={settings.startWithWindows === null} onChange={(startWithWindows) => void updateSettings({ startWithWindows })} label="Start with Windows" hint={settings.startWithWindows === null ? "Not available on this system." : undefined} />
          </Section>

          <Section title="Workspace">
            {workspace ? (
              <>
                <div className="st-ws"><strong>{workspace.name}</strong><code className="st-path">{workspace.path}</code></div>
                <div className="sh-row-actions"><Button variant="danger" icon="x-circle" onClick={() => void closeWs()}>Close workspace</Button></div>
              </>
            ) : <p className="sh-empty">No workspace open.</p>}
          </Section>

          <UpdatesSection />
        </div>
      )}
    </Sheet>
  );
}
