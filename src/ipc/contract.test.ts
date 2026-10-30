/* Checks the JSON the Rust side writes into fixtures/ipc (one `<TypeName>.json` per DTO) against the shapes in types.ts.
   Lenient on purpose: every key the TS type requires must be present in the sample. Skipped while the directory does not exist. */
/// <reference types="node" />
import { existsSync, readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

const dir = join(process.cwd(), "fixtures", "ipc");

/** Required keys per type name (optional keys of types.ts are left out). Keep in step with types.ts. */
const REQUIRED: Record<string, string[]> = {
  HopperError: ["code", "message"],
  KeyValue: ["name", "value", "enabled"],
  RequestSource: ["spec", "key", "hash", "orphaned"],
  RequestDoc: ["id", "name", "method", "url", "pathParams", "query", "headers", "body", "auth"],
  RequestDto: ["doc", "revision", "collection", "folder", "agentAccess", "grantSuspended", "readOnly"],
  WorkspaceInfo: ["id", "name", "path"],
  RecentWorkspace: ["path", "name", "lastOpened"],
  DiscoveredWorkspace: ["path", "name"],
  SpecLinkDto: ["name", "url", "operationCount"],
  CollectionNode: ["slug", "name", "specs", "children"],
  TreeDto: ["collections", "problems"],
  EnvVariable: ["name", "value", "local"],
  ApplicationEnvDto: ["application", "variables", "secrets", "tls", "allowedHosts", "allowedHostsExplicit"],
  StageDto: ["stage", "active", "applications", "revision", "readOnly", "agentRules"],
  PatternRule: ["host", "path"],
  ApplicationEnvInput: ["stage", "application", "variables", "secrets", "tls"],
  LegacyEnvDto: ["stem", "secretNames", "variableNames"],
  MigrationReport: ["stage", "application", "moved", "failed", "notSet", "oldFileKept"],
  SpecImportResult: ["collection", "spec", "title", "version", "operationCount"],
  SearchQuery: ["query"],
  SearchResult: ["hits", "total", "tags", "methods"],
  SendInput: ["runId", "target"],
  Hop: ["status", "url"],
  RunResult: ["runId", "historyId", "requestId", "env", "status", "statusText", "httpVersion", "headers", "kind", "body", "truncated", "timing", "decodedBytes", "redirects", "finalUrl", "insecureTls", "resolvedUrl"],
  HistoryItem: ["id", "requestId", "env", "via", "sentAt", "method", "url", "status", "totalMs", "sizeBytes", "hasRequest"],
  HistoryRow: ["id", "requestId", "env", "via", "sentAt", "method", "url", "status", "totalMs", "sizeBytes", "hasRequest", "name", "saved"],
  CookieDto: ["name", "value", "domain", "path", "secure", "httpOnly"],
  MaskedRequest: ["method", "url", "headers"],
  ApprovalRequest: ["id", "client", "requestName", "requestId", "stage", "application", "request", "reason", "createdAt", "armAfterMs"],
  ActivityEntry: ["id", "at", "client", "tool", "target", "outcome"],
  GrantDto: ["requestId", "requestName", "access", "suspended"],
  McpStatus: ["running"],
  McpSetup: ["claudeCommand", "jsonConfig", "claudeConnection"],
  Settings: ["hotkey", "dockSide", "widthPreset", "theme", "alwaysShowTab", "startWithWindows"],
  RecentItem: ["id", "name", "method", "status"],
};

const files: string[] = existsSync(dir) ? readdirSync(dir).filter((f: string) => f.endsWith(".json")) : [];

describe.skipIf(!existsSync(dir))("ipc fixtures match types.ts", () => {
  files.forEach((file) => it(file, () => {
    const sample: unknown = JSON.parse(readFileSync(join(dir, file), "utf8"));
    const type = file.replace(/\.json$/, "");
    const required = REQUIRED[type];
    if (!required) return; // an enum, alias or type this test does not know yet
    const first = Array.isArray(sample) ? sample[0] : sample;
    if (first === undefined) return; // an empty list says nothing about the shape
    expect(typeof first).toBe("object");
    for (const key of required) expect(first, `${type} lacks "${key}"`).toHaveProperty(key);
  }));
});
