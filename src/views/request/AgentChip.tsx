import type { AgentAccess } from "../../ipc";
import { Icon, cx } from "../../ui";
import { setAgentAccess } from "../../store";
import { Menu, type MenuItem } from "./Menu";

const META: Record<AgentAccess, { label: string; icon: string; tone: string; hint: string }> = {
  ask: { label: "Ask", icon: "hand-palm", tone: "tone-neutral", hint: "The agent asks you before each send" },
  auto: { label: "Auto", icon: "lightning", tone: "tone-ok", hint: "The agent can send without asking, where the environment allows" },
  blocked: { label: "Blocked", icon: "prohibit", tone: "tone-err", hint: "The agent cannot send this request" },
};

export interface AgentChipProps {
  requestId: string | null;
  access: AgentAccess;
  suspended: boolean;
  disabled?: boolean;
}

/** The agent access chip: current grant, and a menu to change it. A suspended grant asks for a review. */
export function AgentChip({ requestId, access, suspended, disabled }: AgentChipProps) {
  const m = META[access];
  const locked = disabled || !requestId;
  const items: MenuItem[] = (Object.keys(META) as AgentAccess[]).map((a) => ({
    label: suspended && a === "auto" ? "Re-allow (Auto)" : META[a].label,
    hint: META[a].hint,
    icon: META[a].icon,
    checked: !suspended && a === access,
    onSelect: () => requestId && void setAgentAccess([requestId], a),
  }));
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
          <span>{suspended ? "Review" : m.label}</span>
          <Icon name="caret-down" className="rv-agent-caret" />
        </>
      }
    />
  );
}
