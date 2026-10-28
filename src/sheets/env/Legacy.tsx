import { useEffect, useState } from "react";
import { toHopperError, type LegacyEnvDto, type MigrationReport, type Stage } from "../../ipc";
import { refreshEnvironments, toast } from "../../store";
import { Button, Select, TextInput } from "../../ui";
import { listLegacyEnvironments, migrateLegacy } from "./api";
import { STAGES } from "./draft";

/** Old single-file environments, each with the stage and application prefilled and a Migrate button. Secret values move inside Rust. */
export function LegacyBanner() {
  const [items, setItems] = useState<LegacyEnvDto[]>([]);
  const [report, setReport] = useState<MigrationReport | null>(null);

  const load = () => listLegacyEnvironments().then(setItems, () => setItems([]));
  useEffect(() => void load(), []);

  if (items.length === 0 && !report) return null;
  return (
    <section className="sh-banner" aria-label="Legacy environments">
      {items.length > 0 && (
        <>
          <h3 className="sh-missing-title">Legacy environments</h3>
          <p className="sh-help">These files come from an older Hopper. Pick where each one goes and migrate it.</p>
          {items.map((i) => (
            <LegacyRow key={i.stem} item={i} onDone={(r) => { setReport(r); void load(); }} />
          ))}
        </>
      )}
      {report && <Report report={report} />}
    </section>
  );
}

function LegacyRow({ item, onDone }: { item: LegacyEnvDto; onDone: (r: MigrationReport) => void }) {
  const [stage, setStage] = useState<Stage>(item.suggestedStage ?? "dev");
  const [application, setApplication] = useState(item.suggestedApplication ?? "");
  const [busy, setBusy] = useState(false);

  const migrate = async () => {
    setBusy(true);
    try {
      onDone(await migrateLegacy(item.stem, stage, application.trim()));
      await refreshEnvironments();
    } catch (e) {
      toast(toHopperError(e).message, "error");
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="sh-row-actions">
      <strong>{item.stem}</strong>
      <Select aria-label={`Stage for ${item.stem}`} value={stage} onChange={(e) => setStage(e.target.value as Stage)}>
        {STAGES.map((s) => <option key={s} value={s}>{s}</option>)}
      </Select>
      <TextInput aria-label={`Application for ${item.stem}`} placeholder="application" value={application} onChange={(e) => setApplication(e.target.value)} />
      <Button size="sm" variant="primary" loading={busy} disabled={!application.trim()} onClick={() => void migrate()}>Migrate</Button>
    </div>
  );
}

function Report({ report }: { report: MigrationReport }) {
  return (
    <div role="status">
      <p className="sh-help">Migrated to {report.stage}/{report.application}. Moved {report.moved.length} secret{report.moved.length === 1 ? "" : "s"}{report.oldFileKept ? "; the old file was kept" : ""}.</p>
      {report.failed.length > 0 && <p className="ui-field-error">Failed: {report.failed.map((f) => `${f.name} (${f.reason})`).join(", ")}</p>}
      {report.notSet.length > 0 && <p className="sh-help">Not set, fill in again: {report.notSet.join(", ")}</p>}
    </div>
  );
}
