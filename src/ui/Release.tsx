import type { ReleaseNote } from "../updater";

const shortDate = (iso: string) => {
  const d = new Date(iso);
  return Number.isNaN(d.getTime()) ? "" : d.toLocaleDateString(undefined, { month: "short", day: "numeric", year: "numeric" });
};

/** One release's notes as plain text: links lose their URL, `* ` lines become list items. */
export function Release({ note }: { note: ReleaseNote }) {
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
