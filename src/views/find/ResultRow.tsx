import { memo, type ReactNode } from "react";
import type { AgentAccess, Method } from "../../ipc";
import { Chip, Icon, cx, type Tone } from "../../ui";
import { ACCESS_LABEL, highlightSegments } from "./model";

export interface RowItem {
  key: string;
  method: Method;
  /** The path (with `matches` to highlight) or, for the saved list, the request name. */
  text: string;
  matches?: number[];
  mono?: boolean;
  sub?: string;
  /** Controller / tag, shown next to the path. */
  tag?: string;
  deprecated?: boolean;
  saved: boolean;
  access?: AgentAccess;
  suspended?: boolean;
  orphaned?: boolean;
  fromSpec?: boolean;
  open: () => void;
}

export const ACCESS_TONE: Record<AgentAccess, Tone> = { ask: "neutral", auto: "ok", blocked: "err" };

export function AccessChip({ access, suspended }: { access: AgentAccess; suspended?: boolean }) {
  if (suspended) return <Chip tone="warn" title="Changed by the agent, review and re-allow">Review</Chip>;
  return <Chip tone={ACCESS_TONE[access]} title={`Agent access: ${ACCESS_LABEL[access]}`}>{ACCESS_LABEL[access]}</Chip>;
}

export function Highlighted({ text, matches }: { text: string; matches?: number[] }): ReactNode {
  return highlightSegments(text, matches ?? []).map((s, i) => (s.hit ? <mark key={i} className="find-hit">{s.text}</mark> : s.text));
}

interface RowProps {
  item: RowItem;
  index: number;
  selected: boolean;
  onHover: (index: number) => void;
}

function ResultRowView({ item, index, selected, onHover }: RowProps) {
  return (
    <li
      id={`find-opt-${index}`}
      role="option"
      aria-selected={selected}
      className={cx("find-row", selected && "is-selected", item.deprecated && "is-deprecated")}
      onMouseMove={() => !selected && onHover(index)}
      onClick={item.open}
    >
      <Chip method={item.method} />
      <div className="find-row-main">
        <div className="find-row-line">
          <span className={cx("find-row-text", item.mono && "is-mono")}>
            <Highlighted text={item.text} matches={item.matches} />
          </span>
          {item.tag && <Chip tone="accent" className="find-tag" title={`Controller: ${item.tag}`}>{item.tag}</Chip>}
        </div>
        {(item.sub || item.deprecated) && (
          <div className="find-row-sub">
            {item.deprecated && <span className="find-deprecated">deprecated</span>}
            {item.sub && <span className="find-row-sub-text">{item.sub}</span>}
          </div>
        )}
      </div>
      <div className="find-row-marks">
        {item.orphaned && <Chip tone="warn" title="The operation is gone from the spec">orphan</Chip>}
        {item.saved ? (
          <>
            <Icon name="bookmark-simple" className="find-saved" />
            {item.access && <AccessChip access={item.access} suspended={item.suspended} />}
          </>
        ) : (
          item.fromSpec && <span className="find-spec" title="An operation from the spec, not saved yet">from spec</span>
        )}
      </div>
    </li>
  );
}

export const ResultRow = memo(ResultRowView);
