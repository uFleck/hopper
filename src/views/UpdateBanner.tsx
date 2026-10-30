import { useState, useSyncExternalStore, type CSSProperties } from "react";
import { Button, Icon } from "../ui";
import { Release } from "../ui/Release";
import { download, getSnapshot, restart, subscribe } from "../updater";
import "../styles/updateBanner.css";

/** Quiet update card at the top of the home screen. Renders nothing unless an update is on offer, downloading, ready or failed. */
export default function UpdateBanner() {
  const upd = useSyncExternalStore(subscribe, getSnapshot);
  const [notesOpen, setNotesOpen] = useState(false);
  if (upd.kind === "idle" || upd.kind === "checking") return null;

  if (upd.kind === "ready") {
    return (
      <div className="ban ready" role="status">
        <span className="ico"><Icon name="arrow-clockwise" /></span>
        <div className="tx"><b>{`${upd.version} is ready`}</b><span>Restart to finish installing</span></div>
        <Button variant="primary" onClick={() => void restart()}>Restart</Button>
      </div>
    );
  }

  if (upd.kind === "error") {
    return (
      <div className="ban err" role="alert">
        <span className="ico"><Icon name="warning" /></span>
        <div className="tx"><b>Update failed</b><span>{upd.message}</span></div>
        <Button onClick={() => void download()}>Try again</Button>
      </div>
    );
  }

  if (upd.kind === "downloading") {
    const pct = upd.progress == null ? null : Math.round(upd.progress * 100);
    return (
      <div className="ban" role="status">
        <span className="ico"><Icon name="circle-notch" className="hdr-spin" /></span>
        <div className="tx"><b>Downloading update</b><span>{pct == null ? "You can keep working" : `${pct}% · you can keep working`}</span></div>
        <Button disabled loading>Downloading</Button>
        <span className="bar"><i style={{ "--progress": `${pct ?? 35}%` } as CSSProperties} /></span>
      </div>
    );
  }

  return (
    <>
      <div className="ban">
        <span className="ico"><Icon name="arrow-circle-up" /></span>
        <div className="tx">
          <b>{`Hopper ${upd.version} is available`}</b>
          <span>
            <button type="button" className="link" aria-expanded={notesOpen} onClick={() => setNotesOpen((o) => !o)}>
              {notesOpen ? "Hide notes" : "What's new"}
            </button>
          </span>
        </div>
        <Button variant="primary" onClick={() => void download()}>Update</Button>
      </div>
      {notesOpen && upd.notes.length > 0 && <div className="ban-notes">{upd.notes.map((n) => <Release key={n.version} note={n} />)}</div>}
    </>
  );
}
