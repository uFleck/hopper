import { useState, useSyncExternalStore } from "react";
import pkg from "../../package.json";
import { Button, Chip } from "../ui";
import { checkForUpdates, download, getSnapshot, restart, subscribe, type ReleaseNote } from "../updater";
import { Note, Section } from "./env/controls";

const shortDate = (iso: string) => {
  const d = new Date(iso);
  return Number.isNaN(d.getTime()) ? "" : d.toLocaleDateString(undefined, { month: "short", day: "numeric", year: "numeric" });
};

/** One release's notes as plain text: links lose their URL, `* ` lines become list items. */
function Release({ note }: { note: ReleaseNote }) {
  const date = shortDate(note.date);
  const lines = note.body
    .split("\n")
    .map((l) => l.trim().replace(/\[([^\]]+)\]\([^)]*\)/g, "$1").replace(/\*\*(.+?)\*\*/g, "$1"))
    .filter((l) => l && !/^Full Changelog/i.test(l));
  return (
    <section className="st-rel">
      <h4>{`v${note.version}${date ? ` · ${date}` : ""}`}</h4>
      <ul>{lines.map((l, i) => <li key={i}>{l.replace(/^[*-]\s+/, "")}</li>)}</ul>
    </section>
  );
}

/** Settings > Updates and about: checks for releases, shows the changelog, downloads, and restarts into the new version. */
export default function UpdatesSection() {
  const upd = useSyncExternalStore(subscribe, getSnapshot);
  const [result, setResult] = useState<"none" | "failed" | null>(null);

  async function checkNow() {
    setResult(null);
    const r = await checkForUpdates();
    if (r === "none" || r === "failed") setResult(r);
  }

  return (
    <Section title="Updates and about">
      {upd.kind === "available" && (
        <>
          <Note>Version {upd.version} is available.</Note>
          <div className="st-rels">{upd.notes.map((n) => <Release key={n.version} note={n} />)}</div>
        </>
      )}
      {upd.kind === "ready" && <Note>Version {upd.version} is ready. Hopper installs it and reopens.</Note>}
      {upd.kind === "error" && <Note tone="danger">Update failed: {upd.message}</Note>}
      {upd.kind === "idle" && result === "none" && <Note>Hopper is up to date.</Note>}
      {upd.kind === "idle" && result === "failed" && <Note tone="warn">Could not check for updates.</Note>}
      <div className="sh-row-actions">
        {upd.kind === "available" && <Button variant="primary" icon="arrow-circle-up" onClick={() => void download()}>Download v{upd.version}</Button>}
        {upd.kind === "downloading" && <Button loading>{upd.progress == null ? "Downloading" : `Downloading ${Math.round(upd.progress * 100)}%`}</Button>}
        {upd.kind === "ready" && <Button variant="primary" icon="arrow-clockwise" onClick={() => void restart()}>Restart to update</Button>}
        {upd.kind === "error" && <Button icon="arrow-clockwise" onClick={() => void download()}>Try again</Button>}
        {(upd.kind === "idle" || upd.kind === "checking") && <Button icon="arrow-clockwise" loading={upd.kind === "checking"} onClick={() => void checkNow()}>Check for updates</Button>}
        <span className="sh-grow" />
        <Chip>Hopper {pkg.version}</Chip>
      </div>
    </Section>
  );
}
