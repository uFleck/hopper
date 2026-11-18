import type { AgentAccess, GrantScope } from "../../ipc";
import { AGENT_MODES, AGENT_MODE_ORDER, agentMode } from "../../agentModes";
import { Icon, cx } from "../../ui";
import { setAgentAccess } from "../../store";
import { Menu, type MenuItem } from "./Menu";

export interface AgentChipProps {
  requestId: string | null;
  access: AgentAccess;
  scope?: GrantScope;
  suspended: boolean;
  disabled?: boolean;
}

/** The agent access chip: current grant, and a menu to change it. A suspended grant asks for a review. */
export function AgentChip({ requestId, access, scope, suspended, disabled }: AgentChipProps) {
  const current = agentMode(access, scope);
  const m = AGENT_MODES[current];
  const locked = disabled || !requestId;
  const items: MenuItem[] = AGENT_MODE_ORDER.map((mode) => {
    const it = AGENT_MODES[mode];
    return {
      label: suspended && mode === "auto" ? "Re-allow (Auto)" : it.label,
      hint: it.warning ?? it.hint,
      icon: it.icon,
      tone: it.warning ? "warn" : undefined,
      checked: !suspended && mode === current,
      onSelect: () => requestId && void setAgentAccess([requestId], it.access, it.scope),
    };
  });
  return (
    <Menu
      ariaLabel={suspended ? "Agent access: changed by the agent, review and re-allow" : `Agent access: ${m.label}`}
      data-tip={locked && !requestId ? "Save the request first to set what the agent may do with it" : suspended ? "Changed by the agent, review and re-allow" : `Agent access: ${m.label}. ${m.hint}`}
      triggerClassName={cx("rv-agent", suspended ? "tone-warn is-suspended" : m.tone)}
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
