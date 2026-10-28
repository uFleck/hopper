import { useMemo, useState, type FormEvent, type ReactNode } from "react";
import type { AgentAccess, CollectionNode, TreeNode } from "../ipc";
import {
  closeSheet,
  createCollection,
  createFolder,
  deleteRequest,
  duplicateRequest,
  moveRequest,
  openRequest,
  setAgentAccess,
  useStore,
} from "../store";
import { Button, Chip, ConfirmDialog, Field, Icon, Select, Sheet, TextInput, confirm } from "../ui";
import { Menu, type MenuItem } from "../views/find/Menu";
import "../styles/find.css";
import { ACCESS_LABEL, flattenRequests, folderOptions, optionKey, requestIdsIn } from "../views/find/model";
import { AccessChip } from "../views/find/ResultRow";
import SpecImportForm from "../views/find/SpecImportForm";
import { CollectionAuth } from "./CollectionAuth";
import { CollectionVariables } from "./CollectionVariables";

type Creating = { collection: string; parent: string[] } | "collection" | null;

const ACCESS_ORDER: AgentAccess[] = ["ask", "auto", "blocked"];
const ACCESS_ICON: Record<AgentAccess, string> = { ask: "hand-waving", auto: "lightning", blocked: "prohibit" };

function MoreButton({ label, ...props }: { label: string } & Record<string, unknown>) {
  return (
    <button {...props} type="button" className="ui-iconbtn ui-iconbtn--sm" aria-label={label} title={label}>
      <Icon name="dots-three" />
    </button>
  );
}

function NameForm({ label, placeholder, onSubmit, onCancel }: { label: string; placeholder: string; onSubmit: (name: string) => Promise<void>; onCancel: () => void }) {
  const [name, setName] = useState("");
  const [busy, setBusy] = useState(false);
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (!name.trim()) return;
    setBusy(true);
    await onSubmit(name.trim());
    setBusy(false);
  };
  return (
    <form className="col-nameform" onSubmit={(e) => void submit(e)} onKeyDown={(e) => e.key === "Escape" && (e.preventDefault(), e.stopPropagation(), onCancel())}>
      <Field label={label}>{(id) => <TextInput id={id} autoFocus value={name} placeholder={placeholder} onChange={(e) => setName(e.target.value)} />}</Field>
      <Button onClick={onCancel}>Cancel</Button>
      <Button type="submit" variant="primary" loading={busy} disabled={!name.trim()}>Create</Button>
    </form>
  );
}

function Notice({ tone = "info", children }: { tone?: "info" | "warn"; children: ReactNode }) {
  return (
    <p className={`col-notice is-${tone}`}>
      <Icon name={tone === "warn" ? "warning" : "info"} />
      <span>{children}</span>
    </p>
  );
}

export default function CollectionsSheet() {
  const open = useStore((s) => s.sheet === "collections");
  if (!open) return null;
  return <CollectionsBody />;
}

function CollectionsBody() {
  const tree = useStore((s) => s.tree);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  const [creating, setCreating] = useState<Creating>(null);
  const [importing, setImporting] = useState<string | null>(null);
  const [moving, setMoving] = useState<{ id: string; name: string } | null>(null);
  const [moveTo, setMoveTo] = useState("");

  const flat = useMemo(() => flattenRequests(tree), [tree]);
  const known = useMemo(() => new Set(flat.map((r) => r.id)), [flat]);
  const places = useMemo(() => folderOptions(tree), [tree]);
  const picked = [...selected].filter((id) => known.has(id));

  const toggle = (id: string) =>
    setSelected((s) => {
      const next = new Set(s);
      if (!next.delete(id)) next.add(id);
      return next;
    });
  const addAll = (ids: string[]) => setSelected((s) => new Set([...s, ...ids]));
  const toggleFolder = (key: string) =>
    setCollapsed((s) => {
      const next = new Set(s);
      if (!next.delete(key)) next.add(key);
      return next;
    });

  const grant = async (ids: string[], access: AgentAccess, scope: string) => {
    if (!ids.length) return;
    if (access === "auto") {
      const ok = await confirm({
        title: `Allow the agent to send ${ids.length} request${ids.length === 1 ? "" : "s"} without asking?`,
        message: `${scope}. Auto requests are sent as soon as the agent asks, except where an environment rule denies them. This grant is user-local: it stays on this machine and is never written into the shared files.`,
        confirmLabel: "Set to Auto",
      });
      if (!ok) return;
    }
    await setAgentAccess(ids, access);
  };

  const requestItems = (r: Extract<TreeNode, { type: "request" }>): MenuItem[] => [
    { key: "open", label: "Open", icon: "arrow-square-out", onSelect: () => (void openRequest(r.id), closeSheet()) },
    { key: "dup", label: "Duplicate", icon: "copy", onSelect: () => (void duplicateRequest(r.id), closeSheet()) },
    {
      key: "move",
      label: "Move to",
      icon: "folder-simple-dashed",
      onSelect: () => {
        const at = flat.find((f) => f.id === r.id);
        setMoveTo(at ? optionKey(at) : (places[0] ? optionKey(places[0]) : ""));
        setMoving({ id: r.id, name: r.name });
      },
    },
    ...ACCESS_ORDER.map((a, i): MenuItem => ({
      key: `a-${a}`,
      label: r.grantSuspended && a === "auto" ? "Re-allow as Auto" : `Agent: ${ACCESS_LABEL[a]}`,
      icon: ACCESS_ICON[a],
      selected: r.agentAccess === a && !r.grantSuspended,
      separated: i === 0,
      onSelect: () => void grant([r.id], a, r.name),
    })),
    { key: "del", label: "Delete", icon: "trash", tone: "danger", separated: true, onSelect: () => void deleteRequest(r.id) },
  ];

  const folderItems = (collection: string, folder: string[], nodes: TreeNode[], name: string): MenuItem[] => {
    const all = requestIdsIn(nodes);
    const gets = requestIdsIn(nodes, (r) => r.method === "GET");
    return [
      { key: "sub", label: "New folder here", icon: "folder-plus", onSelect: () => setCreating({ collection, parent: folder }) },
      { key: "sel", label: `Select all (${all.length})`, icon: "checks", disabled: !all.length, onSelect: () => addAll(all) },
      {
        key: "auto-get",
        label: `Auto for all GET (${gets.length})`,
        icon: "lightning",
        disabled: !gets.length,
        separated: true,
        onSelect: () => void grant(gets, "auto", `All ${gets.length} GET request${gets.length === 1 ? "" : "s"} in ${name}`),
      },
    ];
  };

  const renderNodes = (nodes: TreeNode[], collection: string, folder: string[], depth: number): ReactNode =>
    nodes.map((n) => {
      if (n.type === "folder") {
        const path = [...folder, n.slug];
        const key = `${collection}/${path.join("/")}`;
        const isOpen = !collapsed.has(key);
        const count = requestIdsIn(n.children).length;
        return (
          <li key={key} className="col-node">
            <div className="col-row col-row--folder" style={{ paddingLeft: depth * 14 }}>
              <button type="button" className="col-folder" aria-expanded={isOpen} onClick={() => toggleFolder(key)}>
                <Icon name={isOpen ? "caret-down" : "caret-right"} />
                <Icon name={isOpen ? "folder-open" : "folder"} className="col-folder-icon" />
                <span className="col-name">{n.name}</span>
                <span className="col-count">{count}</span>
              </button>
              <Menu label={`Folder ${n.name}`} items={folderItems(collection, path, n.children, n.name)} trigger={(p) => <MoreButton {...p} label={`Actions for folder ${n.name}`} />} />
            </div>
            {creating && creating !== "collection" && creating.collection === collection && creating.parent.join("/") === path.join("/") && (
              <NameForm label="Folder name" placeholder="Users" onCancel={() => setCreating(null)} onSubmit={async (name) => (await createFolder(collection, path, name), setCreating(null))} />
            )}
            {isOpen && <ul className="col-list">{renderNodes(n.children, collection, path, depth + 1)}</ul>}
          </li>
        );
      }
      return (
        <li key={n.id} className="col-node">
          <div className="col-row" style={{ paddingLeft: depth * 14 }}>
            <input type="checkbox" className="col-check" aria-label={`Select ${n.name}`} checked={selected.has(n.id)} onChange={() => toggle(n.id)} />
            <Chip method={n.method} />
            <button type="button" className="col-req" onClick={() => (void openRequest(n.id), closeSheet())} title={n.name}>
              <span className="col-name">{n.name}</span>
              {n.orphaned && <Chip tone="warn" title="This operation is gone from the spec">orphan</Chip>}
            </button>
            <AccessChip access={n.agentAccess} suspended={n.grantSuspended} />
            <Menu label={`Request ${n.name}`} items={requestItems(n)} trigger={(p) => <MoreButton {...p} label={`Actions for ${n.name}`} />} />
          </div>
          {n.grantSuspended && <p className="col-suspended" style={{ paddingLeft: depth * 14 + 26 }}>Changed by the agent, review and re-allow</p>}
        </li>
      );
    });

  const renderCollection = (c: CollectionNode) => {
    const all = requestIdsIn(c.children);
    const gets = requestIdsIn(c.children, (r) => r.method === "GET");
    return (
      <section key={c.slug} className="col-block" aria-label={`Collection ${c.name}`}>
        <div className="col-head">
          <Icon name="folders" className="col-head-icon" />
          <h3 className="col-head-name">{c.name}</h3>
          <span className="col-count">{all.length}</span>
          <Menu
            label={`Collection ${c.name}`}
            items={[
              ...folderItems(c.slug, [], c.children, c.name).map((it) => (it.key === "sub" ? { ...it, label: "New folder" } : it)),
              { key: "import", label: "Import spec", icon: "download-simple", separated: true, onSelect: () => setImporting(c.slug) },
            ]}
            trigger={(p) => <MoreButton {...p} label={`Actions for collection ${c.name}`} />}
          />
        </div>

        <ul className="col-specs" aria-label="Specs">
          {c.specs.map((sp) => (
            <li key={sp.name} className="col-spec">
              <Icon name="file-code" />
              <span className="col-spec-name" title={sp.url}>{sp.name}</span>
              <span className="col-count">{sp.operationCount} operations</span>
              {sp.importedAt && <span className="col-date">{new Date(sp.importedAt).toLocaleDateString()}</span>}
            </li>
          ))}
          {!c.specs.length && <li className="col-spec col-spec--none">No spec imported</li>}
        </ul>
        <div className="col-block-actions">
          <Button size="sm" icon="download-simple" onClick={() => setImporting(importing === c.slug ? null : c.slug)} aria-expanded={importing === c.slug}>Import spec</Button>
          <Button size="sm" icon="folder-plus" onClick={() => setCreating({ collection: c.slug, parent: [] })}>New folder</Button>
          {gets.length > 0 && (
            <Button size="sm" icon="lightning" title="Set every GET request in this collection to Auto" onClick={() => void grant(gets, "auto", `All ${gets.length} GET requests in ${c.name}`)}>
              Auto for all GET
            </Button>
          )}
        </div>
        <CollectionVariables slug={c.slug} name={c.name} />
        <CollectionAuth slug={c.slug} name={c.name} />
        {importing === c.slug && <SpecImportForm collection={c.slug} onDone={() => setImporting(null)} onCancel={() => setImporting(null)} />}
        {creating && creating !== "collection" && creating.collection === c.slug && creating.parent.length === 0 && (
          <NameForm label="Folder name" placeholder="Users" onCancel={() => setCreating(null)} onSubmit={async (name) => (await createFolder(c.slug, [], name), setCreating(null))} />
        )}
        <ul className="col-list col-list--root">{renderNodes(c.children, c.slug, [], 0)}</ul>
        {!c.children.length && <p className="col-empty">Empty. Open a search result and save it here.</p>}
      </section>
    );
  };

  const bulk = picked.length > 0;
  const problems = tree?.problems ?? [];

  return (
    <>
      <Sheet
        open
        title="Collections"
        onClose={closeSheet}
        actions={<Button size="sm" icon="plus" onClick={() => setCreating("collection")}>Collection</Button>}
        footer={
          bulk ? (
            <div className="col-bulk">
              <div className="col-bulk-head">
                <strong>{picked.length} selected</strong>
                <Button size="sm" variant="ghost" onClick={() => setSelected(new Set())}>Clear</Button>
              </div>
              <div className="col-bulk-actions" role="group" aria-label="Set agent access for the selection">
                <span className="col-bulk-label">Agent access</span>
                {ACCESS_ORDER.map((a) => (
                  <Button key={a} size="sm" icon={ACCESS_ICON[a]} onClick={() => void grant(picked, a, `${picked.length} selected request${picked.length === 1 ? "" : "s"}`).then(() => setSelected(new Set()))}>
                    {ACCESS_LABEL[a]}
                  </Button>
                ))}
              </div>
              <p className="col-bulk-note"><Icon name="user" /> Bulk grants are user-local: they stay on this machine and are not saved in the shared files.</p>
            </div>
          ) : undefined
        }
      >
        {creating === "collection" && (
          <NameForm label="Collection name" placeholder="Account API" onCancel={() => setCreating(null)} onSubmit={async (name) => (await createCollection(name), setCreating(null))} />
        )}

        {problems.length > 0 && (
          <section className="col-problems" aria-label="Problems">
            <h3 className="col-problems-title"><Icon name="warning" /> {problems.length} file{problems.length === 1 ? "" : "s"} could not be loaded</h3>
            <ul>
              {problems.map((p) => (
                <li key={p.path}>
                  <code>{p.path}</code>
                  <span>{p.message}</span>
                </li>
              ))}
            </ul>
          </section>
        )}

        {!bulk && flat.length > 0 && <Notice>Agent access (Ask, Auto, Blocked) is user-local. Tick requests to change several at once.</Notice>}

        {tree?.collections.map(renderCollection)}

        {tree && !tree.collections.length && (
          <div className="col-empty-state">
            <p>No collections yet.</p>
            <Button icon="plus" variant="primary" onClick={() => setCreating("collection")}>New collection</Button>
            <Button icon="download-simple" onClick={() => setImporting("")}>Import a spec</Button>
            {importing === "" && <SpecImportForm onDone={() => setImporting(null)} onCancel={() => setImporting(null)} />}
          </div>
        )}
      </Sheet>

      <ConfirmDialog
        open={!!moving}
        title={moving ? `Move ${moving.name}` : ""}
        confirmLabel="Move"
        message={
          <Field label="Destination">
            {(id) => (
              <Select id={id} value={moveTo} onChange={(e) => setMoveTo(e.target.value)}>
                {places.map((p) => (
                  <option key={optionKey(p)} value={optionKey(p)}>{p.label}</option>
                ))}
              </Select>
            )}
          </Field>
        }
        onCancel={() => setMoving(null)}
        onConfirm={() => {
          const dest = places.find((p) => optionKey(p) === moveTo);
          const id = moving?.id;
          setMoving(null);
          if (dest && id) void moveRequest(id, dest.collection, dest.folder);
        }}
      />
    </>
  );
}
