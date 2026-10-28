import { useState } from "react";
import type { Method, SearchResult } from "../../ipc";
import { cx, methodClass } from "../../ui";

const TAGS_SHOWN = 8;

interface FacetsProps {
  result: SearchResult;
  tag: string | null;
  method: Method | null;
  onTag: (tag: string | null) => void;
  onMethod: (method: Method | null) => void;
}

/** Tag and method counts over the whole match set; clicking one narrows the search, clicking it again lifts it. */
export default function Facets({ result, tag, method, onTag, onMethod }: FacetsProps) {
  const [all, setAll] = useState(false);
  const tags = result.tags;
  const shown = all ? tags : tags.slice(0, TAGS_SHOWN);
  // keep a selected tag visible even when it is past the cut
  const visible = tag && !shown.some((t) => t.name === tag) ? [...shown, ...tags.filter((t) => t.name === tag)] : shown;
  if (!tags.length && !result.methods.length) return null;
  return (
    <div className="facets">
      {result.methods.length > 0 && (
        <div className="facet-row" role="group" aria-label="Filter by method">
          {result.methods.map((m) => (
            <button
              key={m.method}
              type="button"
              className={cx("facet", methodClass(m.method), method === m.method && "is-on")}
              aria-pressed={method === m.method}
              onClick={() => onMethod(method === m.method ? null : m.method)}
            >
              <span className="facet-name facet-method">{m.method}</span>
              <span className="facet-count">{m.count}</span>
            </button>
          ))}
        </div>
      )}
      {tags.length > 0 && (
        <div className="facet-row facet-row--wrap" role="group" aria-label="Filter by tag">
          {visible.map((t) => (
            <button key={t.name} type="button" className={cx("facet", "tone-accent", tag === t.name && "is-on")} aria-pressed={tag === t.name} onClick={() => onTag(tag === t.name ? null : t.name)}>
              <span className="facet-name">{t.name}</span>
              <span className="facet-count">{t.count}</span>
            </button>
          ))}
          {tags.length > TAGS_SHOWN && (
            <button type="button" className="facet facet--more" onClick={() => setAll((a) => !a)}>
              {all ? "fewer" : `+${tags.length - TAGS_SHOWN} more`}
            </button>
          )}
        </div>
      )}
    </div>
  );
}
