import { useEffect, useMemo, useState, type KeyboardEvent, type RefObject } from "react";
import { clearSearch, openOperation, openRequest, openSheet, search, setSearchMethod, setSearchTag, useStore } from "../store";
import { SEARCH_MAX, searchMore } from "../store/specActions";
import { Button, Icon, Spinner } from "../ui";
import Facets from "./find/Facets";
import { flattenRequests, type FlatRequest } from "./find/model";
import { ResultRow, type RowItem } from "./find/ResultRow";
import { NextSteps } from "./FirstRun";
import Pinned from "./Pinned";
import Recent from "./Recent";
import "../styles/find.css";

const SAVED_SHOWN = 50;

function SyntaxHint() {
  return (
    <div className="find-hint">
      <p className="find-hint-title">Search syntax</p>
      <ul>
        <li><code>post users</code> method and words</li>
        <li><code>tag:Admin</code> controller or tag</li>
        <li><code>#GetUserById</code> operation id</li>
        <li><code>path:/api/users/{"{id}"}</code> exact path</li>
      </ul>
    </div>
  );
}

export default function FindView({ inputRef }: { inputRef: RefObject<HTMLInputElement | null> }) {
  const { query, tag, method, loading, result, error } = useStore((s) => s.search);
  const tree = useStore((s) => s.tree);
  const [selected, setSelected] = useState(0);
  const [moreBusy, setMoreBusy] = useState(false);

  const saved = useMemo(() => flattenRequests(tree), [tree]);
  const savedById = useMemo(() => new Map<string, FlatRequest>(saved.map((r) => [r.id, r])), [saved]);
  const searching = !!(query.trim() || tag || method);
  const specCount = tree?.collections.reduce((n, c) => n + c.specs.length, 0) ?? 0;

  const items = useMemo<RowItem[]>(() => {
    if (!searching) {
      return saved.slice(0, SAVED_SHOWN).map((r) => ({
        key: r.id,
        method: r.method,
        text: r.name,
        sub: [r.collection, ...r.folder].join(" / "),
        saved: true,
        access: r.agentAccess,
        suspended: r.grantSuspended,
        orphaned: r.orphaned,
        open: () => void openRequest(r.id),
      }));
    }
    return (result?.hits ?? []).map((h): RowItem => {
      if (h.kind === "saved") {
        const flat = savedById.get(h.requestId);
        return {
          key: `s:${h.requestId}`,
          method: h.method,
          text: h.path,
          matches: h.matches,
          mono: true,
          sub: h.name,
          saved: true,
          access: h.agentAccess,
          suspended: flat?.grantSuspended,
          orphaned: flat?.orphaned,
          open: () => void openRequest(h.requestId),
        };
      }
      const flat = h.savedRequestId ? savedById.get(h.savedRequestId) : undefined;
      return {
        key: `o:${h.spec}:${h.key}`,
        method: h.method,
        text: h.path,
        matches: h.matches,
        mono: true,
        sub: h.summary,
        tag: h.tags[0],
        deprecated: h.deprecated,
        saved: !!h.savedRequestId,
        access: flat?.agentAccess,
        suspended: flat?.grantSuspended,
        fromSpec: !h.savedRequestId,
        open: () => (h.savedRequestId ? void openRequest(h.savedRequestId) : void openOperation(h.collection, h.spec, h.key)),
      };
    });
  }, [searching, saved, savedById, result]);

  useEffect(() => setSelected(0), [result, searching]);
  useEffect(() => {
    document.getElementById(`find-opt-${selected}`)?.scrollIntoView({ block: "nearest" });
  }, [selected, items]);

  const onKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      if (items.length) setSelected((i) => (i + (e.key === "ArrowDown" ? 1 : -1) + items.length) % items.length);
    } else if (e.key === "Enter") {
      e.preventDefault();
      items[selected]?.open();
    } else if (e.key === "Escape" && searching) {
      e.preventDefault();
      clearSearch();
    }
  };

  const total = result?.total ?? 0;
  const shown = result?.hits.length ?? 0;
  const canMore = searching && shown < Math.min(total, SEARCH_MAX);
  const noSpec = !!tree && specCount === 0;

  return (
    <div className="find">
      <div className="find-bar">
        <div className="find-input">
          <Icon name="magnifying-glass" />
          <input
            ref={inputRef}
            type="text"
            role="combobox"
            aria-expanded={items.length > 0}
            aria-controls="find-list"
            aria-activedescendant={items.length ? `find-opt-${selected}` : undefined}
            aria-label="Find a request"
            autoComplete="off"
            spellCheck={false}
            placeholder="Find a request, route or command"
            value={query}
            onChange={(e) => search({ query: e.target.value })}
            onKeyDown={onKeyDown}
            autoFocus
          />
          {loading && <Spinner label="Searching" />}
          {!query && <kbd className="find-kbd" aria-hidden="true">/</kbd>}
        </div>
        <Button icon="folders" onClick={() => openSheet("collections")} aria-label="Browse collections">Browse</Button>
      </div>

      {searching && result && <Facets result={result} tag={tag} method={method} onTag={setSearchTag} onMethod={setSearchMethod} />}
      {(tag || method) && !result && <p className="find-status">Filtering</p>}

      {!searching && <Pinned />}
      {!searching && <Recent />}

      <div className="find-scroll">
        {!searching && !noSpec && items.length === 0 && (
          <div className="find-empty">
            <p>No saved requests yet. Search the {tree?.collections.reduce((n, c) => n + c.specs.reduce((m, s) => m + s.operationCount, 0), 0)} operations from your spec and open one.</p>
            <SyntaxHint />
          </div>
        )}

        {!searching && items.length > 0 && <h2 className="recent-title find-list-title">Saved requests</h2>}

        {searching && error && <p className="find-status is-err" role="alert">{error.message}</p>}

        {searching && !error && result && (
          <p className="find-status" aria-live="polite">
            {total} match{total === 1 ? "" : "es"}
            {shown < total && ` (showing ${shown})`}
          </p>
        )}

        {searching && !error && result && result.hits.length === 0 && !loading && (
          <div className="find-empty">
            <p>Nothing matches{query.trim() ? <> <b>{query.trim()}</b></> : " those filters"}.</p>
            {(tag || method) && (
              <Button size="sm" onClick={() => (setSearchTag(null), setSearchMethod(null))}>Clear filters</Button>
            )}
            <SyntaxHint />
          </div>
        )}

        {items.length > 0 && (
          <ul id="find-list" className="find-list" role="listbox" aria-label={searching ? "Results" : "Saved requests"}>
            {items.map((it, i) => (
              <ResultRow key={it.key} item={it} index={i} selected={i === selected} onHover={setSelected} />
            ))}
          </ul>
        )}

        {/* Below the saved requests, so a workspace with requests but no spec still shows them. */}
        {!searching && noSpec && saved.length === 0 && <NextSteps specImported={false} />}

        {canMore && (
          <div className="find-more">
            <Button
              loading={moreBusy}
              onClick={() => {
                setMoreBusy(true);
                void searchMore().finally(() => setMoreBusy(false));
              }}
            >
              Show more ({shown} of {Math.min(total, SEARCH_MAX)})
            </Button>
            {total > SEARCH_MAX && <span className="find-more-hint">Refine the search to reach the rest.</span>}
          </div>
        )}
      </div>
    </div>
  );
}
