import { useMemo, useRef, useState } from "react";
import { Button, Field, Select, TextInput, useFocusTrap, cx } from "../../ui";
import { editDraft, openSheet, saveRequest, setDraftLocation, useStore } from "../../store";
import { flattenFolders } from "./logic";

/** First save of a draft: pick the collection and folder (from the tree) and confirm the name. */
export function SaveDialog({ onClose }: { onClose: () => void }) {
  const tree = useStore((s) => s.tree);
  const draftName = useStore((s) => s.draft?.doc.name ?? "");
  const draftCollection = useStore((s) => s.draft?.collection ?? null);
  const draftFolder = useStore((s) => s.draft?.folder);
  const collections = tree?.collections ?? [];
  const [name, setName] = useState(draftName);
  const [collection, setCollection] = useState(draftCollection && collections.some((c) => c.slug === draftCollection) ? draftCollection : (collections[0]?.slug ?? ""));
  const [folder, setFolder] = useState<string[]>(draftFolder ?? []);
  const [busy, setBusy] = useState(false);
  const ref = useRef<HTMLFormElement>(null);
  const nameRef = useRef<HTMLInputElement>(null);
  useFocusTrap(ref, true, nameRef);

  const current = collections.find((c) => c.slug === collection);
  const folders = useMemo(() => (current ? flattenFolders(current.children, current.name) : []), [current]);
  const key = folder.join("/");

  const submit = async () => {
    if (!current || !name.trim()) return;
    setBusy(true);
    editDraft({ name: name.trim() });
    setDraftLocation(current.slug, folder);
    const ok = await saveRequest();
    setBusy(false);
    if (ok) onClose();
  };

  return (
    <div className="ui-scrim" onMouseDown={(e) => e.stopPropagation()}>
      <form
        ref={ref}
        className="ui-dialog rv-savedlg"
        role="dialog"
        aria-modal="true"
        aria-label="Save request"
        onSubmit={(e) => (e.preventDefault(), void submit())}
        onKeyDown={(e) => e.key === "Escape" && (e.preventDefault(), e.stopPropagation(), onClose())}
      >
        <h3 className="ui-dialog-title">Save request</h3>
        {collections.length === 0 ? (
          <>
            <p className="ui-dialog-msg">There is no collection to save into yet. Create one in the Collections sheet, then save again.</p>
            <div className="ui-dialog-actions">
              <Button onClick={onClose}>Close</Button>
              <Button variant="primary" onClick={() => (onClose(), openSheet("collections"))}>Open collections</Button>
            </div>
          </>
        ) : (
          <>
            <Field label="Name">{(id) => <TextInput id={id} ref={nameRef} value={name} onChange={(e) => setName(e.target.value)} invalid={!name.trim()} />}</Field>
            {collections.length > 1 && (
              <Field label="Collection">
                {(id) => (
                  <Select id={id} value={collection} onChange={(e) => (setCollection(e.target.value), setFolder([]))}>
                    {collections.map((c) => (
                      <option key={c.slug} value={c.slug}>{c.name}</option>
                    ))}
                  </Select>
                )}
              </Field>
            )}
            <div className="ui-field">
              <span className="ui-field-label" id="rv-folder-label">Folder</span>
              <div className="rv-folders" role="radiogroup" aria-labelledby="rv-folder-label">
                {folders.map((f) => {
                  const on = f.path.join("/") === key;
                  return (
                    <button key={f.path.join("/")} type="button" role="radio" aria-checked={on} className={cx("rv-folder", on && "is-on")} style={{ paddingLeft: 8 + f.depth * 14 }} onClick={() => setFolder(f.path)}>
                      <i className={`ph ph-${f.depth === 0 ? "stack" : "folder"}`} aria-hidden="true" />
                      <span>{f.name}</span>
                    </button>
                  );
                })}
              </div>
            </div>
            <div className="ui-dialog-actions">
              <Button onClick={onClose}>Cancel</Button>
              <Button type="submit" variant="primary" loading={busy} disabled={!name.trim()}>Save here</Button>
            </div>
          </>
        )}
      </form>
    </div>
  );
}
