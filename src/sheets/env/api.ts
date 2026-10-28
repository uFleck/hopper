/* Store-independent helpers for the environments and agent sheets: thin wrappers over `invoke`. They reject with a HopperError. */
import { invoke, type ApplicationEnvInput, type PatternRule, type Stage } from "../../ipc";

export interface AppScope {
  stage: Stage;
  application: string;
}

export const saveEnvironment = (env: ApplicationEnvInput) => invoke("environment_save", { env });
export const deleteEnvironment = (scope: AppScope) => invoke("environment_delete", scope);
export const setSecretValue = (scope: AppScope, name: string, value: string) => invoke("secret_set", { ...scope, name, value });
export const clearSecretValue = (scope: AppScope, name: string) => invoke("secret_clear", { ...scope, name });
/** The only path on which a secret value reaches the UI. The caller must hide it again after 10 s. */
export const revealSecretValue = (scope: AppScope, name: string) => invoke("secret_reveal", { ...scope, name });
export const saveAgentRules = (env: Stage, deny: boolean, safeReads: boolean, patterns: PatternRule[]) =>
  invoke("agent_env_rules_set", { env, deny, safeReads, patterns });
export const listLegacyEnvironments = () => invoke("environment_legacy_list");
export const migrateLegacy = (stem: string, stage: Stage, application: string) => invoke("environment_migrate_legacy", { stem, stage, application });
export const listGrants = () => invoke("agent_grants_list");
export const getMcpSetup = () => invoke("mcp_setup");
export const connectClaudeCode = () => invoke("mcp_connect_claude");

export const REVEAL_SECONDS = 10;
