import type { AgentAccess, GrantScope } from "../../ipc";
import { Icon, cx } from "../../ui";
import { setAgentAccess } from "../../store";
import { MODES, MODE_ORDER, accessOf, modeOf } from "../../sheets/agent/modes";
import { Menu, type MenuItem } from "./Menu";

export interface AgentChipProps {
  requestId: string | null;
  access: AgentAccess;
  scope: GrantScope;
  suspended: boolean;
  disabled?: boolean;
}

/** The agent access chip: current grant, and a menu to change it. A suspended grant asks for a review. */
export function AgentChip({ requestId, access, scope, suspended, disabled }: AgentChipProps) {
  const mode = modeOf(access, scope);
  const m = MODES[mode];
  const locked = disabled || !requestId;
  const items: MenuItem[] = MODE_ORDER.map((k) => ({
    label: suspended && k === "auto" ? "Re-allow (Auto)" : MODES[k].label,
    hint: MODES[k].hint,
    title: MODES[k].risk,
    icon: MODES[k].icon,
    tone: MODES[k].risk ? "warn" : "default",
    checked: !suspended && k === mode,
    onSelect: () => {
      if (!requestId) return;
      const next = accessOf(k);
      void setAgentAccess([requestId], next.access, next.scope);
    },
  }));
  return (
    <Menu
      ariaLabel={suspended ? "Agent access: changed by the agent, review and re-allow" : `Agent access: ${m.label}`}
      data-tip={locked && !requestId ? "Save the request first to set what the agent may do with it" : suspended ? "Changed by the agent, review and re-allow" : `Agent access: ${m.label}. ${m.hint}${m.risk ? `. ${m.risk}` : ""}`}
      triggerClassName={cx("rv-agent", suspended ? "tone-warn is-suspended" : `tone-${m.tone}`)}
      disabled={locked}
      items={items}
      trigger={
        <>
          <Icon name={suspended ? "warning" : m.icon} />
          <span>{suspended ? "Review" : m.short}</span>
          <Icon name="caret-down" className="rv-agent-caret" />
        </>
      }
    />
  );
}
