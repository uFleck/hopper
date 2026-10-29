import { describe, expect, it } from "vitest";
import type { Stage, StageDto } from "../../ipc";
import { buildMatrix, gapsIn, groupByApplication, isRawValue, nameProblem, secretGaps } from "./matrix";
import { progress, setupItems } from "./setup";

const stage = (name: Stage, active: boolean, apps: StageDto["applications"]): StageDto => ({
  stage: name, active, applications: apps, revision: "r", readOnly: false, agentRules: { deny: false, safeReads: true, patterns: [] },
});
const app = (application: string, variables: [string, string][], secrets: [string, "set" | "missing"][]) => ({
  application,
  variables: variables.map(([name, value]) => ({ name, value, local: false })),
  secrets: secrets.map(([name, status]) => ({ name, provider: "keyring", status })),
  tls: { insecureSkipVerify: false }, allowedHosts: [], allowedHostsExplicit: false,
});

const stages = [
  stage("local", false, [app("acct", [["baseUrl", "http://localhost"]], [["apiToken", "set"]])]),
  stage("dev", false, []),
  stage("rc", true, [app("acct", [], [["apiToken", "missing"]]), app("billing", [], [["key", "missing"], ["other", "set"]])]),
  stage("production", false, [app("acct", [["tenant", "p9"]], [["apiToken", "set"]])]),
];

describe("buildMatrix", () => {
  const rows = buildMatrix([{ name: "baseUrl", value: "https://api" }, { name: "tenant", value: "t1" }], stages, "acct");

  it("lists variables then secrets, with the default and what each stage does with it", () => {
    expect(rows.map((r) => r.name)).toEqual(["baseUrl", "tenant", "apiToken"]);
    const base = rows[0];
    expect(base.defaultValue).toBe("https://api");
    expect(base.cells.local).toEqual({ kind: "value", value: "http://localhost", local: false });
    expect(base.cells.rc).toEqual({ kind: "inherit", value: "https://api" });
    expect(rows[1].cells.production).toMatchObject({ kind: "value", value: "p9" });
  });

  it("gives secrets a status per stage and flags the stage that never declared it", () => {
    const secret = rows[2];
    expect(secret.secret).toBe(true);
    expect(secret.defaultValue).toBeNull();
    expect(secret.cells).toMatchObject({ local: { kind: "set" }, dev: { kind: "undeclared" }, rc: { kind: "missing" }, production: { kind: "set" } });
  });

  it("counts what still needs a value in a stage", () => {
    expect(gapsIn(rows, "rc").map((r) => r.name)).toEqual(["apiToken"]);
    expect(gapsIn(rows, "local")).toEqual([]);
    expect(gapsIn(buildMatrix([], stages, "acct"), "dev").map((r) => r.name)).toEqual(["baseUrl", "tenant", "apiToken"]);
  });
});

describe("secretGaps", () => {
  it("finds secrets without a value per stage, for collections nobody opened", () => {
    expect(secretGaps(stages, "rc")).toEqual([{ application: "acct", name: "apiToken", declared: true }, { application: "billing", name: "key", declared: true }]);
    expect(secretGaps(stages, "dev")).toEqual([{ application: "acct", name: "apiToken", declared: false }, { application: "billing", name: "key", declared: false }, { application: "billing", name: "other", declared: false }]);
    expect(secretGaps(stages, "local").map((g) => g.application)).toContain("billing");
    expect(groupByApplication(secretGaps(stages, "rc")).map((g) => [g.application, g.gaps.length])).toEqual([["acct", 1], ["billing", 1]]);
  });
});

describe("new variable names", () => {
  const rows = buildMatrix([{ name: "baseUrl", value: "x" }], stages, "acct");
  it("rejects bad and duplicate names, accepts a free one", () => {
    expect(nameProblem("", rows)).toBeNull();
    expect(nameProblem("1abc", rows)).toMatch(/Letters/);
    expect(nameProblem("baseUrl", rows)).toMatch(/Already defined/);
    expect(nameProblem("apiToken", rows)).toMatch(/Already defined/);
    expect(nameProblem("tenantId", rows)).toBeNull();
  });

  it("tells a raw credential from a variable", () => {
    expect(isRawValue("eyJhbGciOi")).toBe(true);
    expect(isRawValue("{{apiToken}}")).toBe(false);
    expect(isRawValue("Bearer {{t}}")).toBe(false);
    expect(isRawValue("  ")).toBe(false);
  });
});

describe("setup checklist", () => {
  const rows = buildMatrix([{ name: "baseUrl", value: "https://api" }], stages, "acct");
  const by = (items: ReturnType<typeof setupItems>, key: string) => items.find((i) => i.key === key)!;

  it("flags a bearer secret with no value in the active stage and counts progress", () => {
    const items = setupItems({ operations: 24, rows, auth: { type: "bearer", token: "{{apiToken}}" }, active: "rc" });
    expect(by(items, "auth")).toMatchObject({ state: "todo", go: { tab: "auth" } });
    expect(by(items, "values").state).toBe("todo");
    expect(by(items, "base").state).toBe("ready");
    expect(progress(items)).toEqual({ done: 2, total: 4 });
  });

  it("is ready when the secret is set, and treats no auth and no spec as optional", () => {
    const ok = setupItems({ operations: 0, rows, auth: { type: "bearer", token: "{{apiToken}}" }, active: "local" });
    expect(by(ok, "auth").state).toBe("ready");
    expect(by(ok, "spec").state).toBe("optional");
    expect(by(setupItems({ operations: 0, rows, auth: { type: "none" }, active: "local" }), "auth").state).toBe("optional");
  });

  it("asks for the credential to be moved out of the collection file", () => {
    expect(by(setupItems({ operations: 0, rows, auth: { type: "bearer", token: "eyJraw" }, active: "local" }), "auth")).toMatchObject({ state: "todo", detail: expect.stringMatching(/collection file/) });
  });

  it("asks for a baseUrl when there is none", () => {
    expect(by(setupItems({ operations: 0, rows: [], auth: undefined, active: "rc" }), "base")).toMatchObject({ state: "todo", go: { label: "Add" } });
  });
});
