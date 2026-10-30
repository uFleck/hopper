import { useSyncExternalStore } from "react";
import { openSheet } from "../store";
import { Icon } from "../ui";
import { getSnapshot, restart, subscribe } from "../updater";

/** Header button that only exists while there is something to do about an update: read the notes, restart, or see the failure. */
export default function UpdateButton() {
  const upd = useSyncExternalStore(subscribe, getSnapshot);
  if (upd.kind === "idle" || upd.kind === "checking") return null;

  if (upd.kind === "ready") {
    return (
      <button type="button" className="ui-iconbtn hdr-upd is-ready" aria-label={`Restart to update to v${upd.version}`} data-tip={`Restart to update to v${upd.version}`} onClick={() => void restart()}>
        <Icon name="arrow-circle-up" />
      </button>
    );
  }
  const label =
    upd.kind === "available" ? `Update to v${upd.version}` : upd.kind === "downloading" ? (upd.progress == null ? "Downloading update" : `Downloading update ${Math.round(upd.progress * 100)}%`) : "Update failed";
  return (
    <button type="button" className={`ui-iconbtn hdr-upd${upd.kind === "error" ? " is-bad" : ""}`} aria-label={label} data-tip={label} onClick={() => openSheet("settings")}>
      <Icon name={upd.kind === "downloading" ? "circle-notch" : "arrow-circle-up"} className={upd.kind === "downloading" ? "hdr-spin" : undefined} />
      {upd.kind === "available" && <span className="hdr-warn-dot hdr-menu-dot" aria-hidden="true" />}
    </button>
  );
}
