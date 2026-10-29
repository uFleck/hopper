import { useEffect, useRef, useState } from "react";
import { inTauri } from "./ipc";
import { onPanel, panelEvent, panelMode, type PanelModeInfo } from "./panel/control";
import CollectionsSheet from "./sheets/CollectionsSheet";
import Sheets from "./sheets/Sheets";
import { startUpdateChecks } from "./updater";
import { initStore, markNoticesSeen, selectActiveEnv, useStore } from "./store";
import { Busy, ConfirmHost, ToastHost } from "./ui";
import ApprovalPrompt from "./views/ApprovalPrompt";
import FindView from "./views/FindView";
import { EnvGuard } from "./views/EnvPicker";
import FloatingHopper from "./views/FloatingHopper";
import FirstRun from "./views/FirstRun";
import Header from "./views/Header";
import RequestView from "./views/RequestView";
import Tray from "./views/Tray";
import "./styles/panel.css";
import "./styles/ui.css";

/** Hopper, stuck to a screen edge. Collapsed it is a handle (a pill when something needs attention); open it is the panel.
    The search box is the home screen; the request view replaces it while a request is open. */
export default function App() {
  const searchBox = useRef<HTMLInputElement>(null);
  const workspace = useStore((s) => s.workspace);
  const ready = useStore((s) => s.ready);
  const route = useStore((s) => s.route);
  const envKind = useStore(selectActiveEnv) ?? undefined;
  const [info, setInfo] = useState<PanelModeInfo>({ mode: "open", right: true });
  // The panel plays its exit while the window is still full size; the backend then shrinks it and reports the new mode.
  const [leaving, setLeaving] = useState(false);
  const collapsed = info.mode === "tab" || info.mode === "compact";

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && !e.defaultPrevented && panelEvent("dismiss");
    window.addEventListener("keydown", onKey);
    // A summon takes focus, so the search box is ready to type into.
    const off = onPanel({
      summon: () => searchBox.current?.focus(),
      collapsing: () => setLeaving(true),
      mode: (next) => {
        setLeaving(false);
        setInfo(next);
      },
    });
    void panelMode().then(setInfo);
    startUpdateChecks();
    const dispose = initStore();
    return () => {
      window.removeEventListener("keydown", onKey);
      off();
      dispose();
    };
  }, []);

  useEffect(() => {
    document.documentElement.dataset.mode = info.mode;
    document.documentElement.dataset.side = info.right ? "right" : "left";
    document.documentElement.dataset.shell = inTauri() ? "app" : "web";
    if (info.mode === "open") markNoticesSeen();
  }, [info.mode, info.right]);

  return (
    <>
      {collapsed && <FloatingHopper info={info} />}
      <main className={collapsed ? "panel is-collapsed" : leaving ? "panel is-leaving" : "panel"} data-env={envKind} onMouseDown={() => panelEvent("clicked")}>
        <Header />
        {ready && workspace && <EnvGuard />}

        {!ready ? (
          <Busy label="Loading" />
        ) : !workspace ? (
          <FirstRun />
        ) : route === "request" ? (
          <RequestView />
        ) : (
          <FindView inputRef={searchBox} />
        )}

        {ready && workspace && <Tray />}

        <CollectionsSheet />
        <Sheets />
        <ApprovalPrompt />

        <ToastHost />
        <ConfirmHost />
      </main>
    </>
  );
}
