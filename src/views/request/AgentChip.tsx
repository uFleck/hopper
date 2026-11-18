import type { AgentAccess } from "../../ipc";
import { Icon, confirm, cx } from "../../ui";
import { setAgentAccess } from "../../store";
import { ACCESS_LABEL, ACCESS_SHORT, LOOSE_AUTO_RISK } from "../find/model";
import { Menu, type MenuItem } from "./Menu";

const META: Record<AgentAccess, { icon: string; tone: string; hint: string }> = {
  ask: { icon: "hand-palm", tone: "tone-neutral", hint: "The agent asks you before each send" },
  auto: { icon: "lightning", tone: "tone-ok", hint: "The agent can send it unchanged without asking, where the environment allows" },
  auto_params: { icon: "lightning", tone: "tone-ok", hint: "The agent can change path and query values and send without asking" },
  auto_trusted: { icon: "lightning", tone: "tone-warn", hint: "The agent can edit anything in the request and send without asking" },
  blocked: { icon: "prohibit", tone: "tone-err", hint: "The agent cannot send this request" },
};

/** Sets agent access; the loose Auto modes first spell out their prompt-injection risk. */
export async function chooseAgentAccess(requestIds: string[], access: AgentAccess, name: string): Promise<boolean> {
  const risk = LOOSE_AUTO_RISK[access];
  if (risk) {
    const ok = await confirm({
      title: `${ACCESS_LABEL[access]} for ${name}?`,
      message: `${risk} You can set it back to Ask at any time in the Agent sheet.`,
      confirmLabel: "Allow",
      tone: "danger",
    });
    if (!ok) return false;
  }
  await setAgentAccess(requestIds, access);
  return true;
}

export interface AgentChipProps {
  requestId: string | null;
  access: AgentAccess;
  suspended: boolean;
  disabled?: boolean;
  name?: string;
}

/** The agent access chip: current grant, and a menu to change it. A suspended grant asks for a review. */
export function AgentChip({ requestId, access, suspended, disabled, name = "this request" }: AgentChipProps) {
  const m = META[access];
  const label = ACCESS_LABEL[access];
  const locked = disabled || !requestId;
  const items: MenuItem[] = (Object.keys(META) as AgentAccess[]).map((a) => ({
    label: suspended && a.startsWith("auto") ? `Re-allow (${ACCESS_LABEL[a]})` : ACCESS_LABEL[a],
    hint: META[a].hint,
    icon: META[a].icon,
    checked: !suspended && a === access,
    onSelect: () => requestId && void chooseAgentAccess([requestId], a, name),
  }));
  return (
    <Menu
      ariaLabel={suspended ? "Agent access: changed by the agent, review and re-allow" : `Agent access: ${label}`}
      data-tip={locked && !requestId ? "Save the request first to set what the agent may do with it" : suspended ? "Changed by the agent, review and re-allow" : `Agent access: ${label}. ${m.hint}`}
      triggerClassName={cx("rv-agent", suspended ? "tone-warn is-suspended" : m.tone)}
      disabled={locked}
      items={items}
      trigger={
        <>
          <Icon name={suspended ? "warning" : m.icon} />
          <span>{suspended ? "Review" : ACCESS_SHORT[access]}</span>
          <Icon name="caret-down" className="rv-agent-caret" />
        </>
      }
    />
  );
}
