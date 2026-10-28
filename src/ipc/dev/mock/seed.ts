/* Seed data for the dev mock: one workspace, one collection, a spec with ~35 operations across many `User*` controllers. */
import type {
  ActivityEntry,
  CookieDto,
  StageDto,
  Method,
  RecentWorkspace,
  RequestDoc,
  RequestDto,
  Settings,
  WorkspaceInfo,
} from "../../types";

export const COLLECTION = "account-api";
export const COLLECTION_NAME = "Account API";
export const SPEC = "account-openapi";

export const workspace: WorkspaceInfo = { id: "ws-umbler-apis", name: "Umbler APIs", path: "C:\\Work\\umbler-apis" };

export const recentWorkspaces: RecentWorkspace[] = [
  { path: workspace.path, name: workspace.name, lastOpened: "2026-10-25T18:02:00Z" },
  { path: "C:\\Work\\payments-sandbox", name: "Payments sandbox", lastOpened: "2026-10-12T09:30:00Z" },
];

export interface MockOperation {
  key: string;
  method: Method;
  path: string;
  summary: string;
  tag: string;
  deprecated?: boolean;
}

/* [method, path, operationId, controller (tag), summary] */
const OPS: [Method, string, string, string, string, boolean?][] = [
  ["GET", "/api/users", "UserAdmin_List", "UserAdmin", "List users"],
  ["POST", "/api/users", "UserAdmin_Create", "UserAdmin", "Create a user"],
  ["GET", "/api/users/{id}", "UserAdmin_Get", "UserAdmin", "Get a user by id"],
  ["PUT", "/api/users/{id}", "UserAdmin_Update", "UserAdmin", "Update a user"],
  ["DELETE", "/api/users/{id}", "UserAdmin_Delete", "UserAdmin", "Delete a user"],
  ["POST", "/api/users/{id}/password-reset", "UserAdmin_ResetPassword", "UserAdmin", "Send a password reset"],
  ["POST", "/api/users/{id}/suspend", "UserAdmin_Suspend", "UserAdmin", "Suspend a user"],
  ["GET", "/api/users/{id}/profile", "UserProfile_Get", "UserProfile", "Get the user profile"],
  ["PATCH", "/api/users/{id}/profile", "UserProfile_Patch", "UserProfile", "Update the user profile"],
  ["PUT", "/api/users/{id}/avatar", "UserProfile_SetAvatar", "UserProfile", "Upload an avatar"],
  ["GET", "/api/users/{id}/preferences", "UserProfile_Preferences", "UserProfile", "Get preferences"],
  ["GET", "/api/users/{id}/notifications", "UserNotifications_List", "UserNotifications", "List notifications"],
  ["POST", "/api/users/{id}/notifications/read", "UserNotifications_MarkRead", "UserNotifications", "Mark notifications read"],
  ["DELETE", "/api/users/{id}/notifications/{notificationId}", "UserNotifications_Delete", "UserNotifications", "Delete a notification"],
  ["PUT", "/api/users/{id}/notifications/settings", "UserNotifications_Settings", "UserNotifications", "Update notification settings"],
  ["GET", "/api/users/{id}/sessions", "UserSessions_List", "UserSessions", "List active sessions"],
  ["DELETE", "/api/users/{id}/sessions/{sessionId}", "UserSessions_Revoke", "UserSessions", "Revoke a session"],
  ["DELETE", "/api/users/{id}/sessions", "UserSessions_RevokeAll", "UserSessions", "Revoke all sessions"],
  ["GET", "/api/invites", "UserInvites_List", "UserInvites", "List invites"],
  ["POST", "/api/invites", "UserInvites_Create", "UserInvites", "Create an invite"],
  ["POST", "/api/invites/{code}/accept", "UserInvites_Accept", "UserInvites", "Accept an invite"],
  ["DELETE", "/api/invites/{code}", "UserInvites_Revoke", "UserInvites", "Revoke an invite"],
  ["GET", "/api/roles", "UserRoles_List", "UserRoles", "List roles"],
  ["POST", "/api/roles", "UserRoles_Create", "UserRoles", "Create a role"],
  ["GET", "/api/users/{id}/roles", "UserRoles_ForUser", "UserRoles", "Roles of a user"],
  ["PUT", "/api/users/{id}/roles/{roleId}", "UserRoles_Assign", "UserRoles", "Assign a role"],
  ["DELETE", "/api/users/{id}/roles/{roleId}", "UserRoles_Unassign", "UserRoles", "Remove a role"],
  ["GET", "/api/users/{id}/api-keys", "UserApiKeys_List", "UserApiKeys", "List API keys"],
  ["POST", "/api/users/{id}/api-keys", "UserApiKeys_Create", "UserApiKeys", "Create an API key"],
  ["DELETE", "/api/users/{id}/api-keys/{keyId}", "UserApiKeys_Revoke", "UserApiKeys", "Revoke an API key"],
  ["POST", "/api/auth/login", "Auth_Login", "Auth", "Sign in"],
  ["POST", "/api/auth/refresh", "Auth_Refresh", "Auth", "Refresh a token"],
  ["GET", "/api/organizations/{orgId}/members", "Organization_Members", "Organization", "List members"],
  ["GET", "/api/billing/invoices", "Billing_Invoices", "Billing", "List invoices"],
  ["GET", "/api/legacy/users/export", "Legacy_ExportUsers", "Legacy", "Export users (CSV)", true],
];

export const operations: MockOperation[] = OPS.map(([method, path, key, tag, summary, deprecated]) => ({
  key,
  method,
  path,
  summary,
  tag,
  deprecated: !!deprecated,
}));

const kv = (name: string, value: string, enabled = true) => ({ name, value, enabled });

function doc(id: string, name: string, method: Method, url: string, extra: Partial<RequestDoc> = {}): RequestDoc {
  return {
    id,
    name,
    method,
    url,
    pathParams: {},
    query: [],
    headers: [],
    body: { type: "none" },
    auth: { type: "inherit" },
    ...extra,
  };
}

const src = (key: string, orphaned = false) => ({ spec: SPEC, key, hash: `h-${key}`, orphaned });

function dto(d: RequestDoc, folder: string[], extra: Partial<RequestDto> = {}): RequestDto {
  return {
    doc: d,
    revision: `rev-${d.id}-1`,
    collection: COLLECTION,
    folder,
    agentAccess: "ask",
    grantSuspended: false,
    readOnly: false,
    ...extra,
  };
}

export function seedRequests(): RequestDto[] {
  return [
    dto(doc("01HX0000000000000000000001", "List users", "GET", "{{baseUrl}}/api/users", {
      query: [kv("page", "1"), kv("pageSize", "25"), kv("search", "", false)],
      source: src("UserAdmin_List"),
    }), [], { agentAccess: "auto" }),
    dto(doc("01HX0000000000000000000002", "Get user", "GET", "{{baseUrl}}/api/users/{id}", {
      pathParams: { id: "{{userId}}" },
      source: src("UserAdmin_Get"),
    }), ["users"], { agentAccess: "auto" }),
    dto(doc("01HX0000000000000000000003", "Create invite", "POST", "{{baseUrl}}/api/invites", {
      headers: [kv("Content-Type", "application/json")],
      body: { type: "json", content: '{\n  "email": "new.hire@umbler.com",\n  "role": "Member"\n}' },
      source: src("UserInvites_Create"),
    }), ["users", "invites"], { agentAccess: "blocked" }),
    dto(doc("01HX0000000000000000000004", "Reset password", "POST", "{{baseUrl}}/api/users/{id}/password-reset", {
      pathParams: { id: "{{userId}}" },
      notes: "The agent edited the body of this one.",
      source: src("UserAdmin_ResetPassword"),
    }), ["users", "admin"], { agentAccess: "ask", grantSuspended: true }),
    dto(doc("01HX0000000000000000000005", "Export users (old)", "GET", "{{baseUrl}}/api/legacy/users/export", {
      source: src("Legacy_RemovedExport", true),
    }), [], { agentAccess: "ask" }),
    dto(doc("01HX0000000000000000000006", "Slow report", "GET", "{{baseUrl}}/api/reports/slow", {
      timeoutMs: 30000,
    }), ["reports"], { agentAccess: "auto" }),
    dto(doc("01HX0000000000000000000007", "Missing page", "GET", "{{baseUrl}}/api/missing"), ["reports"]),
    dto(doc("01HX0000000000000000000008", "Terms of service", "GET", "{{baseUrl}}/terms", {
      auth: { type: "none" },
    }), []),
    dto(doc("01HX0000000000000000000009", "Needs a token", "GET", "{{baseUrl}}/api/me?key={{adminKey}}"), ["users"]),
  ];
}

/** The four fixed stages, each with one `account` application section. `dev` has no section yet. */
export function seedEnvironments(): StageDto[] {
  const agentRules = { deny: false, safeReads: true, patterns: [] };
  return [
    {
      stage: "local",
      active: true,
      readOnly: false,
      agentRules,
      revision: "env-local-1",
      applications: [
        {
          application: "account",
          variables: [
            { name: "baseUrl", value: "http://localhost:5080", local: false },
            { name: "userId", value: "42", local: true },
          ],
          secrets: [{ name: "apiToken", provider: "keyring", status: "set" }],
          tls: { insecureSkipVerify: true },
          allowedHosts: ["localhost"],
          allowedHostsExplicit: false,
        },
      ],
    },
    { stage: "dev", active: false, readOnly: false, agentRules, revision: null, applications: [] },
    {
      stage: "rc",
      active: false,
      readOnly: false,
      agentRules,
      revision: "env-rc-1",
      applications: [
        {
          application: "account",
          variables: [
            { name: "baseUrl", value: "https://account-staging.umbler.dev", local: false },
            { name: "userId", value: "1007", local: false },
          ],
          secrets: [
            { name: "apiToken", provider: "keyring", status: "set" },
            { name: "adminKey", provider: "keyring", status: "missing" },
          ],
          tls: { insecureSkipVerify: false },
          allowedHosts: ["account-staging.umbler.dev", "*.umbler.dev"],
          allowedHostsExplicit: true,
        },
      ],
    },
    {
      stage: "production",
      active: false,
      readOnly: false,
      agentRules: {
        deny: false,
        safeReads: true,
        patterns: [{ method: "GET", host: "account.umbler.com", path: "/api/users/*" }],
      },
      revision: "env-production-1",
      applications: [
        {
          application: "account",
          variables: [
            { name: "baseUrl", value: "https://account.umbler.com", local: false },
            { name: "userId", value: "9001", local: false },
          ],
          secrets: [
            { name: "apiToken", provider: "keyring", status: "set" },
            { name: "adminKey", provider: "keyring", status: "set" },
          ],
          tls: { insecureSkipVerify: false },
          allowedHosts: ["account.umbler.com"],
          allowedHostsExplicit: false,
        },
      ],
    },
  ];
}

export const settings: Settings = {
  hotkey: "Ctrl+Alt+H",
  dockSide: "right",
  widthPreset: 640,
  theme: "dark",
  alwaysShowTab: true,
  startWithWindows: true,
};

export const cookies: CookieDto[] = [
  { name: "session", value: "9f2c...", domain: "account-staging.umbler.dev", path: "/", secure: true, httpOnly: true },
  { name: "locale", value: "pt-BR", domain: "localhost", path: "/", secure: false, httpOnly: false },
];

export function seedActivity(): ActivityEntry[] {
  const at = (minAgo: number) => new Date(Date.now() - minAgo * 60_000).toISOString();
  return [
    { id: "act-1", at: at(42), client: "Claude Code", tool: "hopper_search_routes", target: "user roles", outcome: "ok" },
    { id: "act-2", at: at(40), client: "Claude Code", tool: "hopper_send", target: "Get user", outcome: "ok", allowedBy: "request grant" },
    { id: "act-3", at: at(12), client: "Claude Code", tool: "hopper_send", target: "Create invite", outcome: "denied", detail: "Blocked for agents" },
  ];
}
