import { useEffect, useMemo, useState } from "react";
import type { Auth } from "../ipc";
import { loadCollectionAuth, saveCollectionAuth, toast, useStore } from "../store";
import { Button, Icon } from "../ui";
import { AuthSection, LABEL } from "../views/request/AuthSection";
import { varInfo } from "../views/request/logic";

/** The authentication every request in a collection inherits. Folded by default; edits are saved with one button. */
export function CollectionAuth({ slug, name }: { slug: string; name: string }) {
  const loaded = useStore((s) => s.collectionAuth[slug]);
  const [open, setOpen] = useState(false);
  const [edited, setEdited] = useState<Auth | null>(null);
  const [busy, setBusy] = useState(false);
  const info = useMemo(() => varInfo(null), []);

  useEffect(() => {
    if (open) void loadCollectionAuth(slug);
  }, [open, slug]);

  // The draft follows what is on disk until the user edits it.
  const saved = loaded?.auth;
  const draft: Auth = edited ?? saved ?? { type: "inherit" };
  const dirty = edited !== null && JSON.stringify(edited) !== JSON.stringify(saved ?? { type: "inherit" });
  const showChip = saved !== undefined && saved.type !== "none" && saved.type !== "inherit";

  const save = async () => {
    setBusy(true);
    const ok = await saveCollectionAuth(slug, draft, loaded?.revision ?? null);
    setBusy(false);
    if (ok) {
      setEdited(null);
      toast(`Saved authentication of ${name}`, "success");
    }
  };

  return (
    <div className="col-vars">
      <button type="button" className="col-vars-head" aria-expanded={open} onClick={() => setOpen(!open)}>
        <Icon name={open ? "caret-down" : "caret-right"} />
        <span>Authentication</span>
        {showChip && saved && <span className="col-count">{LABEL[saved.type]}</span>}
      </button>
      {open && (
        <div className="col-vars-body">
          <p className="col-vars-hint">
            Every request in {name} that is set to Inherit uses this. Put the token in a secret of the environment and reference it here as {"{{name}}"}. Changing it pauses Auto-approve agent grants for this collection until you approve again.
          </p>
          <AuthSection scope="collection" auth={draft} onChange={setEdited} info={info} readOnly={false} />
          {dirty && (
            <div className="col-block-actions">
              <Button size="sm" variant="primary" loading={busy} onClick={() => void save()}>Save</Button>
              <Button size="sm" variant="ghost" disabled={busy} onClick={() => setEdited(null)}>Discard</Button>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
