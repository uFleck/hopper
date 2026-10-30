import type { ReactNode } from "react";
import type { CollectionNode, TreeNode } from "../../ipc";
import { collectionKey, folderKey, openRequest, toggleNode, useOpenNodes } from "../../store";
import { Chip, Icon } from "../../ui";
import { requestIdsIn } from "./model";
import { AccessChip } from "./ResultRow";

/** A collection or folder header. The open state lives in `treeOpen`, so it survives restarts and workspace switches. */
function Head({ id, name, count, isOpen, icon, onToggle }: { id: string; name: string; count: number; isOpen: boolean; icon?: string; onToggle: () => void }) {
  return (
    <button type="button" className="tree-head" aria-expanded={isOpen} aria-controls={id} onClick={onToggle}>
      <Icon name={isOpen ? "caret-down" : "caret-right"} className="tree-caret" />
      {icon && <Icon name={isOpen ? "folder-open" : icon} className="tree-icon" />}
      <span className="tree-name">{name}</span>
      <span className="tree-count">{count}</span>
    </button>
  );
}

function Nodes({ nodes, collection, path, open }: { nodes: TreeNode[]; collection: string; path: string[]; open: ReadonlySet<string> }): ReactNode {
  return nodes.map((n) => {
    if (n.type === "request") {
      const flagged = n.grantSuspended || n.agentAccess !== "ask";
      return (
        <li key={n.id}>
          <button type="button" className="tree-req" onClick={() => void openRequest(n.id)} data-tip={n.name}>
            <Chip method={n.method} />
            <span className="tree-name">{n.name}</span>
            {n.orphaned && <Chip tone="warn" tip="The operation is gone from the spec">orphan</Chip>}
            {flagged && <AccessChip access={n.agentAccess} suspended={n.grantSuspended} />}
          </button>
        </li>
      );
    }
    const next = [...path, n.slug];
    const key = folderKey(collection, next);
    const isOpen = open.has(key);
    return (
      <li key={key} className="tree-folder">
        <Head id={`tree-${key}`} name={n.name} count={requestIdsIn(n.children).length} isOpen={isOpen} icon="folder" onToggle={() => toggleNode(key)} />
        {isOpen && (
          <ul id={`tree-${key}`} className="tree-list">
            <Nodes nodes={n.children} collection={collection} path={next} open={open} />
          </ul>
        )}
      </li>
    );
  });
}

/** Every saved request as collection > folder > request accordions. */
export default function SavedTree({ collections }: { collections: CollectionNode[] }) {
  const open = useOpenNodes();
  return (
    <ul className="tree" aria-label="Saved requests">
      {collections.map((c) => {
        const key = collectionKey(c.slug);
        const isOpen = open.has(key);
        const empty = requestIdsIn(c.children).length === 0;
        return (
          <li key={key} className="tree-collection">
            <Head id={`tree-${key}`} name={c.name} count={requestIdsIn(c.children).length} isOpen={isOpen} icon="folders" onToggle={() => toggleNode(key)} />
            {isOpen && (
              <ul id={`tree-${key}`} className="tree-list">
                {empty ? <li className="tree-empty">No saved requests</li> : <Nodes nodes={c.children} collection={c.slug} path={[]} open={open} />}
              </ul>
            )}
          </li>
        );
      })}
    </ul>
  );
}
