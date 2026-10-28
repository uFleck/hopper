/* `request_send` for the mock: variable resolution, simulated latency with progress events, cancel, plausible bodies. */
import { bus } from "../../bus";
import type { ApplicationEnvDto, HopperError, RequestDoc, RunResult, SendInput } from "../../types";

/** The active stage's application section, named by its stage. */
export type ResolvedEnv = { name: string } & Pick<ApplicationEnvDto, "variables" | "secrets" | "tls" | "allowedHosts">;

const cancelled = new Set<string>();
const live = new Set<string>();

export const cancelRun = (runId: string) => {
  if (live.has(runId)) cancelled.add(runId);
};

export const err = (code: string, message: string, extra: Partial<HopperError> = {}): HopperError => ({ code, message, ...extra });

const VAR = /\{\{\s*([\w.-]+)\s*\}\}/g;

function variablesIn(doc: RequestDoc): string[] {
  const text = [
    doc.url,
    ...Object.values(doc.pathParams),
    ...doc.query.filter((q) => q.enabled).flatMap((q) => [q.name, q.value]),
    ...doc.headers.filter((h) => h.enabled).flatMap((h) => [h.name, h.value]),
    doc.body.type === "json" || doc.body.type === "text" || doc.body.type === "xml" ? doc.body.content : "",
    doc.auth.type === "bearer" ? doc.auth.token : "",
  ].join("\n");
  return [...new Set([...text.matchAll(VAR)].map((m) => m[1]))];
}

/** Resolves `{{vars}}` against the environment, or throws the same errors the Rust resolver would. */
export function resolve(doc: RequestDoc, env: ResolvedEnv | undefined, overrides: Record<string, string> = {}) {
  const values = new Map<string, string>();
  const missingSecrets: string[] = [];
  env?.variables.forEach((v) => values.set(v.name, v.value));
  env?.secrets.forEach((s) => (s.status === "set" ? values.set(s.name, `••••`) : missingSecrets.push(s.name)));
  Object.entries(overrides).forEach(([k, v]) => values.set(k, v));

  const used = variablesIn(doc);
  const secretless = used.filter((n) => missingSecrets.includes(n) && !(n in overrides));
  if (secretless.length) {
    throw err("secret_missing", `The secret ${secretless.join(", ")} has no value in ${env?.name}.`, {
      hint: "Set it in the environments sheet.",
      details: { names: secretless, env: env?.name },
    });
  }
  const unresolved = used.filter((n) => !values.has(n));
  if (unresolved.length) {
    throw err("unresolved_variables", `Unresolved variables: ${unresolved.join(", ")}.`, {
      hint: env ? `Define them in ${env.name} or pick another environment.` : "Pick an environment first.",
      details: { names: unresolved },
    });
  }
  const sub = (s: string) => s.replace(VAR, (_, n: string) => values.get(n) ?? "");
  let url = sub(doc.url);
  for (const [k, v] of Object.entries(doc.pathParams)) url = url.replace(`{${k}}`, sub(v));
  const enabled = doc.query.filter((q) => q.enabled && q.name);
  if (enabled.length) {
    url += (url.includes("?") ? "&" : "?") + enabled.map((q) => `${sub(q.name)}=${encodeURIComponent(sub(q.value))}`).join("&");
  }
  return { url };
}

const sleep = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));

function sample(doc: RequestDoc, url: string): { status: number; statusText: string; kind: RunResult["kind"]; body: string; type: string } {
  const path = url.replace(/^https?:\/\/[^/]+/, "").split("?")[0];
  if (path.includes("missing")) {
    return { status: 404, statusText: "Not Found", kind: "json", type: "application/json", body: JSON.stringify({ type: "https://httpstatuses.io/404", title: "Not Found", status: 404, detail: `No route for ${path}` }, null, 2) };
  }
  if (path.includes("terms") || path.endsWith(".html")) {
    return {
      status: 200,
      statusText: "OK",
      kind: "html",
      type: "text/html; charset=utf-8",
      body: "<!doctype html>\n<html lang=\"en\">\n<head><title>Terms of service</title></head>\n<body>\n  <h1>Terms of service</h1>\n  <p>Be kind to the API.</p>\n</body>\n</html>",
    };
  }
  if (doc.method === "DELETE") return { status: 204, statusText: "No Content", kind: "text", type: "text/plain", body: "" };
  if (doc.method === "POST" && !path.includes("password-reset")) {
    return { status: 201, statusText: "Created", kind: "json", type: "application/json", body: JSON.stringify({ id: "inv_8f2c1a", email: "new.hire@umbler.com", role: "Member", createdAt: new Date().toISOString() }, null, 2) };
  }
  if (path.endsWith("/users") || path.endsWith("/roles") || path.endsWith("/invites")) {
    const items = Array.from({ length: 12 }, (_, i) => ({ id: 1000 + i, name: `User ${i + 1}`, email: `user${i + 1}@umbler.com`, active: i % 5 !== 0 }));
    return { status: 200, statusText: "OK", kind: "json", type: "application/json", body: JSON.stringify({ page: 1, pageSize: 25, total: 128, items }, null, 2) };
  }
  if (path.includes("slow")) {
    return { status: 200, statusText: "OK", kind: "json", type: "application/json", body: JSON.stringify({ report: "monthly-usage", rows: 48210, generatedInMs: 2410 }, null, 2) };
  }
  return { status: 200, statusText: "OK", kind: "json", type: "application/json", body: JSON.stringify({ id: 42, name: "Ada Lovelace", email: "ada@umbler.com", roles: ["Admin", "Member"], profile: { locale: "pt-BR", timezone: "America/Sao_Paulo" } }, null, 2) };
}

export async function simulateSend(input: SendInput, doc: RequestDoc, env: ResolvedEnv | undefined, requestId: string | null): Promise<RunResult> {
  const { runId } = input;
  const { url } = resolve(doc, env, input.overrides);
  const host = url.replace(/^https?:\/\//, "").split(/[/?#]/)[0];
  if (host === "evil.example.com" && input.confirmedHost !== host) {
    throw err("confirm_unlisted_host", `${host} is not in the allowed hosts of ${env?.name}.`, { details: { host } });
  }
  live.add(runId);
  const emit = (phase: "preparing" | "sending" | "receiving" | "done") => bus.emit("run://progress", { runId, phase });
  const slow = url.includes("slow");
  const stop = () => {
    if (cancelled.delete(runId)) {
      live.delete(runId);
      throw err("cancelled", "The request was cancelled.");
    }
  };
  try {
    emit("preparing");
    await sleep(60);
    stop();
    emit("sending");
    // Poll in slices so a cancel lands quickly.
    const ttfb = slow ? 2200 : 180 + Math.round(Math.random() * 220);
    for (let t = 0; t < ttfb; t += 50) {
      await sleep(50);
      stop();
    }
    emit("receiving");
    await sleep(slow ? 250 : 60);
    stop();
    const s = sample(doc, url);
    const result: RunResult = {
      runId,
      historyId: `hist-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 6)}`,
      requestId,
      env: env?.name ?? "none",
      status: s.status,
      statusText: s.statusText,
      httpVersion: "HTTP/1.1",
      headers: [
        ["content-type", s.type],
        ["date", new Date().toUTCString()],
        ["server", "Kestrel"],
        ["x-request-id", Math.random().toString(16).slice(2, 14)],
      ],
      kind: s.kind,
      body: s.body,
      truncated: false,
      timing: { ttfbMs: ttfb, downloadMs: slow ? 250 : 60, totalMs: ttfb + (slow ? 250 : 60) + 60 },
      transferredBytes: s.body.length,
      decodedBytes: s.body.length,
      redirects: [],
      finalUrl: url,
      insecureTls: !!env?.tls.insecureSkipVerify && url.startsWith("https"),
      resolvedUrl: url,
    };
    emit("done");
    return result;
  } finally {
    live.delete(runId);
    cancelled.delete(runId);
  }
}
