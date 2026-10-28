import { useState, type FormEvent } from "react";
import { useStore } from "../../store";
import { importSpec } from "../../store/specActions";
import { Button, Field, Segmented, Select, TextInput } from "../../ui";

/** In-panel spec import: a URL or a file path, into an existing collection (or a new one when there is none). */
export default function SpecImportForm({ onDone, onCancel, collection }: { onDone?: () => void; onCancel?: () => void; collection?: string }) {
  const collections = useStore((s) => s.tree?.collections) ?? [];
  const [kind, setKind] = useState<"url" | "file">("url");
  const [source, setSource] = useState("");
  const [target, setTarget] = useState(collection ?? collections[0]?.slug ?? "");
  const [newName, setNewName] = useState("API");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    const value = source.trim();
    if (!value) return setError(kind === "url" ? "Enter the spec URL." : "Enter the path of the spec file.");
    if (kind === "url" && !/^https?:\/\//i.test(value)) return setError("The URL must start with http:// or https://.");
    setError(null);
    setBusy(true);
    const dest = collections.length ? target || collections[0].slug : { create: newName.trim() || "API" };
    const result = await importSpec(dest, kind === "url" ? { url: value } : { file: value });
    setBusy(false);
    if (result) {
      setSource("");
      onDone?.();
    }
  };

  return (
    <form className="import-form" onSubmit={(e) => void submit(e)}>
      <Segmented
        ariaLabel="Spec source"
        value={kind}
        onChange={(v) => {
          setKind(v);
          setError(null);
        }}
        options={[
          { value: "url", label: "URL" },
          { value: "file", label: "File" },
        ]}
      />
      <Field label={kind === "url" ? "OpenAPI URL" : "OpenAPI file path"} error={error} hint={kind === "url" ? "swagger.json or openapi.yaml, e.g. https://localhost:5001/swagger/v1/swagger.json" : "JSON or YAML, e.g. C:\\specs\\account.json"}>
        {(id) => (
          <TextInput id={id} mono value={source} invalid={!!error} placeholder={kind === "url" ? "https://" : "C:\\path\\to\\openapi.json"} onChange={(e) => setSource(e.target.value)} autoFocus />
        )}
      </Field>
      {collections.length > 1 && (
        <Field label="Into collection">
          {(id) => (
            <Select id={id} value={target} onChange={(e) => setTarget(e.target.value)}>
              {collections.map((c) => (
                <option key={c.slug} value={c.slug}>{c.name}</option>
              ))}
            </Select>
          )}
        </Field>
      )}
      {collections.length === 0 && (
        <Field label="New collection name">{(id) => <TextInput id={id} value={newName} onChange={(e) => setNewName(e.target.value)} />}</Field>
      )}
      <div className="import-actions">
        {onCancel && <Button onClick={onCancel}>Cancel</Button>}
        <Button type="submit" variant="primary" icon="download-simple" loading={busy}>Import spec</Button>
      </div>
    </form>
  );
}
