/**
 * Every collection binding, every empty read (Sol ctx r13 P2-1).
 *
 * The rows come from `collection-catalog.cjs`, the same array the prelude
 * loads. A new row is a new case. A call site that builds a collection without
 * `__rhinoLocalCollectionGrant("id")` fails the source scan, and an id with no
 * call site fails it too. On a size-zero seed each operation is either a
 * named unmeasured fault or a grant that names its capture. Building the
 * binding does not fault: the unread-seed controls cover that.
 */
import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";
import vm from "node:vm";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { CaseRunError, runCase } from "../../src/bindings/run.ts";
import type { CompleteRecordedEffects, Engine, JsonValue, ScriptContext } from "../../src/case/types.ts";
import { defineCase, validateCase } from "../../src/case/validate.ts";
import { expectOwnedEmulationFailure } from "../../src/emulation.ts";
import { packageRoot } from "../../src/paths.ts";
import { RhinoRunner } from "../../src/runner.ts";

const OPS = [
  "size", "length", "string", "isEmpty", "contains", "indexOf", "forEach",
  "toArray", "toArraySize", "toArrayLength", "toArrayJson",
  "json", "concat", "get", "index", "iteration",
] as const;
type Op = (typeof OPS)[number];

interface Grant {
  capture?: string;
  surface?: boolean;
  length?: boolean;
  json?: boolean;
  concat?: boolean;
  contains?: boolean;
  indexOf?: boolean;
  isEmpty?: boolean;
  forEach?: boolean;
  toArray?: boolean;
  add?: boolean;
  size?: boolean;
  get?: boolean;
  index?: boolean;
  iteration?: boolean;
  lengthAbsent?: boolean;
  toArrayLength?: boolean;
  toArraySize?: boolean;
  toArrayJson?: boolean;
  toArraySurface?: boolean;
}

interface Row {
  id: string;
  context: ScriptContext;
  engine: Engine;
  kind: "list" | "set";
  expr: string;
  faultPrefix: string;
  toArrayCall: "decision" | "guarded" | "stub";
  grant: Grant | null;
  given: Record<string, JsonValue>;
  empty?: boolean;
  size?: string;
}

const catalogPath = join(packageRoot, "src", "bindings", "rhino", "collection-catalog.cjs");
const bindingsDir = join(packageRoot, "src", "bindings", "rhino");

function loadCatalog(): Row[] {
  const sandbox: { __rhinoLocalCollectionCatalog?: Row[] } = {};
  vm.createContext(sandbox);
  vm.runInContext(readFileSync(catalogPath, "utf8"), sandbox, { filename: catalogPath });
  const rows = sandbox.__rhinoLocalCollectionCatalog;
  if (!Array.isArray(rows) || rows.length === 0) {
    throw new Error("collection catalog did not load");
  }
  return rows;
}

const catalog = loadCatalog();

/** Catalog rows are vm-realm objects. `defineCase` rejects those; a JSON clone is a plain seed. */
function plainGiven(given: Record<string, JsonValue>): Record<string, JsonValue> {
  return JSON.parse(JSON.stringify(given)) as Record<string, JsonValue>;
}

function grantCalls(source: string): string[][] {
  const found: string[][] = [];
  const needle = "__rhinoLocalCollectionGrant(";
  let from = 0;
  while (from < source.length) {
    const at = source.indexOf(needle, from);
    if (at < 0) break;
    let i = at + needle.length;
    let depth = 1;
    let buf = "";
    while (i < source.length && depth > 0) {
      const c = source[i]!;
      if (c === "(") depth += 1;
      else if (c === ")") depth -= 1;
      if (depth > 0) buf += c;
      i += 1;
    }
    found.push([...buf.matchAll(/"([^"]+)"/g)].map((match) => match[1]!));
    from = i;
  }
  return found;
}

/** A collection constructor must sit near a catalog grant, or be marked collection-kind. */
function unaccountedConstructors(source: string, file: string): string[] {
  const out: string[] = [];
  const re = /__rhinoLocal(?:CtxList|CtxJavaSet|ListView)\(/g;
  for (const match of source.matchAll(re)) {
    const at = match.index ?? 0;
    const window = source.slice(Math.max(0, at - 2500), at);
    if (window.includes("__rhinoLocalCollectionGrant(") || window.includes("collection-kind")) continue;
    const line = source.slice(0, at).split("\n").length;
    out.push(`${file}:${line}`);
  }
  return out;
}

type Expectation =
  | { kind: "value"; value: string }
  | { kind: "fault" }
  | { kind: "script"; needle: string };

/**
 * One harness cause per distinct refusal. `toArray().size()` never runs when
 * `toArray()` itself is refused, and for...in of an ungranted empty list reads
 * `[0]`, so those operations share a cause. The harness dedupes identical
 * causes; declaring one expectation per operation would leave the extras
 * unmatched.
 */
function faultKey(row: Row, op: Op): string | null {
  if (expectation(row, op).kind !== "fault") return null;
  if ((op === "toArraySize" || op === "toArrayLength" || op === "toArrayJson") && expectation(row, "toArray").kind === "fault") {
    return "toArray";
  }
  if (op === "iteration" && row.kind === "list" && expectation(row, "index").kind === "fault") return "index";
  // Rhino builds "Cannot find function … in object" by calling String() on the
  // receiver. An ungranted empty set's toString throws, so that is the error
  // indexOf and get surface, the same cause as the string read.
  if ((op === "indexOf" || op === "get") && row.kind === "set" && expectation(row, "string").kind === "fault") {
    return "string";
  }
  return op;
}

function allows(row: Row, route: string): boolean {
  const grant = row.grant;
  if (grant === null) return false;
  if (route === "length") return grant.length === true || (grant.surface === true && row.kind === "list");
  if (route === "index" || route === "iteration") return grant.index === true || grant.iteration === true;
  return grant[route as keyof Grant] === true;
}

function expectation(row: Row, op: Op): Expectation {
  const grant = row.grant;
  // A set has no indexOf or get. When String() is granted, Rhino finishes the
  // TypeError ("Cannot find function … in object []"), which is the measured
  // legacy requestedScopes indexOf. When String() faults, that fault is what
  // the call throws, because the message format calls toString.
  if (row.kind === "set" && op === "indexOf") {
    return allows(row, "surface")
      ? { kind: "script", needle: "Cannot find function indexOf" }
      : { kind: "fault" };
  }
  if (row.kind === "set" && op === "get") {
    return allows(row, "surface")
      ? { kind: "script", needle: "Cannot find function get" }
      : { kind: "fault" };
  }
  if (row.kind === "set" && op === "index") return { kind: "script", needle: "no public instance field" };
  if (op === "length" && grant?.lengthAbsent === true) return { kind: "value", value: "undefined" };
  if (op === "toArray" && (row.toArrayCall === "guarded" || allows(row, "toArray"))) {
    return { kind: "value", value: "object" };
  }
  if (op === "toArrayLength" && grant?.toArrayLength === true && row.toArrayCall !== "stub" && (row.toArrayCall === "guarded" || allows(row, "toArray"))) {
    return { kind: "value", value: "0" };
  }
  if (op === "toArraySize" && grant?.toArraySize === true && row.toArrayCall !== "stub" && (row.toArrayCall === "guarded" || allows(row, "toArray"))) {
    return { kind: "value", value: "0" };
  }
  if (op === "toArrayJson" && grant?.toArrayJson === true && row.toArrayCall !== "stub" && (row.toArrayCall === "guarded" || allows(row, "toArray"))) {
    return { kind: "value", value: "[]" };
  }
  const values: Partial<Record<Op, string>> = {
    size: "0",
    length: "0",
    string: "[]",
    isEmpty: "true",
    contains: "false",
    indexOf: "-1",
    forEach: "0",
    json: "[]",
    concat: "0",
    get: "undefined",
    index: "undefined",
    iteration: "",
  };
  if (op === "string" && allows(row, "surface")) return { kind: "value", value: "[]" };
  if (op === "json" && allows(row, "json")) return { kind: "value", value: "[]" };
  if (op === "length" && allows(row, "length")) return { kind: "value", value: "0" };
  if (values[op] !== undefined && allows(row, op)) return { kind: "value", value: values[op]! };
  return { kind: "fault" };
}

/** Sol ctx r13's 26 green cases. Each must be a fault, not a grant. */
const SOL_CASES: { id: string; ops: Op[] }[] = [
  { id: "validate.next.allowedScopes", ops: ["size"] },
  { id: "validate.next.availableScopes", ops: ["size"] },
  { id: "validate.next.defaultScopes", ops: ["size"] },
  { id: "oidc.next.scopes", ops: ["size"] },
  { id: "oidc.next.scopes", ops: ["toArraySize"] },
  { id: "mayact.next.scopes", ops: ["size"] },
  { id: "validate.next.allowedScopes", ops: ["toArraySize"] },
  { id: "validate.legacy.allowedScopes", ops: ["toArraySize"] },
  { id: "token.legacy.scopes", ops: ["string"] },
  { id: "token.legacy.scopes", ops: ["size"] },
  { id: "token.legacy.scopes", ops: ["isEmpty"] },
  { id: "token.legacy.scopes", ops: ["toArrayLength"] },
  { id: "token.legacy.getScope", ops: ["string"] },
  { id: "token.legacy.getScope", ops: ["size"] },
  { id: "token.legacy.getScope", ops: ["isEmpty"] },
  { id: "token.legacy.getScope", ops: ["toArrayLength"] },
  { id: "oidc.legacy.scopes", ops: ["string"] },
  { id: "oidc.legacy.scopes", ops: ["size"] },
  { id: "token.legacy.allowedGrantTypes", ops: ["string", "size"] },
  { id: "token.legacy.allowedScopes", ops: ["string", "size"] },
  { id: "token.legacy.allowedResponseTypes", ops: ["string", "size"] },
  { id: "oidc.legacy.allowedGrantTypes", ops: ["string", "size"] },
  { id: "oidc.legacy.allowedScopes", ops: ["string", "size"] },
  { id: "oidc.legacy.allowedResponseTypes", ops: ["string", "size"] },
  { id: "social.legacy.rawProfile.keys", ops: ["string", "size"] },
  { id: "social.next.rawProfile.keys", ops: ["size"] },
];

function reads(expr: string): string {
  return [
    "var report = {};",
    "function read(name, fn) {",
    "  try { report[name] = \"ok:\" + String(fn()); }",
    "  catch (err) { report[name] = \"throw:\" + String(err).split(\" in object \")[0]; }",
    "}",
    `read("size", function () { return ${expr}.size(); });`,
    `read("length", function () { return ${expr}.length; });`,
    `read("string", function () { return String(${expr}); });`,
    `read("isEmpty", function () { return ${expr}.isEmpty(); });`,
    `read("contains", function () { return ${expr}.contains("x"); });`,
    `read("indexOf", function () { return ${expr}.indexOf("x"); });`,
    `read("forEach", function () { var n = 0; ${expr}.forEach(function () { n += 1; }); return n; });`,
    `read("toArray", function () { return typeof ${expr}.toArray(); });`,
    `read("toArraySize", function () { return ${expr}.toArray().size(); });`,
    `read("toArrayLength", function () { return ${expr}.toArray().length; });`,
    `read("toArrayJson", function () { return JSON.stringify(${expr}.toArray()); });`,
    `read("json", function () { return JSON.stringify(${expr}); });`,
    `read("concat", function () { return [].concat(${expr}).length; });`,
    `read("get", function () { return ${expr}.get(0); });`,
    `read("index", function () { return ${expr}[0]; });`,
    `read("iteration", function () { var s = ""; var k; for (k in ${expr}) { s += String(${expr}[k]); } return s; });`,
  ].join("\n");
}

function scriptFor(row: Row): string {
  const body = reads(row.expr);
  const packed = `${body}\nvar packed = JSON.stringify(report);`;
  switch (row.context) {
    case "OAUTH2_VALIDATE_SCOPE":
      return `function validateAccessTokenScope() {\n${body}\nreturn [JSON.stringify(report)];\n}`;
    case "OAUTH2_ACCESS_TOKEN_MODIFICATION":
      return `${packed}\naccessToken.setField("probe", packed);`;
    case "OIDC_CLAIMS":
      if (row.engine === "legacy") {
        return `${packed}\nvar m = new java.util.LinkedHashMap();\nm.put("probe", packed);\nnew org.forgerock.oauth2.core.UserInfoClaims(m, null);`;
      }
      return `${packed}\n({values:{probe:packed}});`;
    case "OAUTH2_MAY_ACT":
      // Legacy setMayAct takes a JsonValue. A plain object is the next-gen shape.
      if (row.engine === "legacy") {
        return `${packed}\nvar m = new java.util.LinkedHashMap();\nm.put("probe", packed);\ntoken.setMayAct(new org.forgerock.json.JsonValue(m));`;
      }
      return `${packed}\ntoken.setMayAct({probe:packed});`;
    case "SOCIAL_IDP_PROFILE_TRANSFORMATION":
      return `${packed}\n({probe:packed});`;
    case "CONFIG_PROVIDER_NODE":
      return `${packed}\nconfig = {probe:packed};`;
    default:
      throw new Error(`no script wrapper for ${row.context}`);
  }
}

function allow(context: ScriptContext): Record<string, boolean> {
  switch (context) {
    case "OAUTH2_VALIDATE_SCOPE": return { validateScope: true };
    case "OAUTH2_ACCESS_TOKEN_MODIFICATION": return { accessToken: true };
    case "OIDC_CLAIMS": return { oidc: true };
    case "OAUTH2_MAY_ACT": return { mayAct: true };
    case "SOCIAL_IDP_PROFILE_TRANSFORMATION": return { socialIdp: true };
    case "CONFIG_PROVIDER_NODE": return { configProvider: true };
    default: return {};
  }
}

function reportOf(row: Row, effects: CompleteRecordedEffects): string {
  switch (row.context) {
    case "OAUTH2_VALIDATE_SCOPE": return effects.validateScope?.granted?.[0] ?? "";
    case "OAUTH2_ACCESS_TOKEN_MODIFICATION": return String(effects.accessToken?.final.fields.probe ?? "");
    case "OIDC_CLAIMS": return String(effects.oidc?.userinfo.probe ?? "");
    case "OAUTH2_MAY_ACT": {
      const claim = effects.mayAct?.mayAct;
      return claim !== null && typeof claim === "object" && !Array.isArray(claim) ? String(claim.probe ?? "") : "";
    }
    case "SOCIAL_IDP_PROFILE_TRANSFORMATION": return String(effects.socialIdp?.profile?.probe ?? "");
    case "CONFIG_PROVIDER_NODE": return String(effects.configProvider?.config?.probe ?? "");
    default: return "";
  }
}

describe("empty-read conformance (Sol ctx r13 P2-1)", () => {
  it("derives every binding from the catalog and the grant call sites", () => {
    const ids = catalog.map((row) => row.id);
    expect(new Set(ids).size).toBe(ids.length);
    const files = readdirSync(bindingsDir).filter((name) => name.endsWith(".cjs") && name !== "collection-catalog.cjs");
    const calls = files.flatMap((name) => grantCalls(readFileSync(join(bindingsDir, name), "utf8")));
    const wired = calls.flat();
    expect(wired.filter((id) => !ids.includes(id))).toEqual([]);
    expect(ids.filter((id) => !wired.includes(id)).sort()).toEqual([]);
    const missed = files.flatMap((name) => unaccountedConstructors(readFileSync(join(bindingsDir, name), "utf8"), name));
    expect(missed).toEqual([]);
  });

  it("records Sol's 26 cases as faults, each citing no capture", () => {
    expect(SOL_CASES).toHaveLength(26);
    const byId = new Map(catalog.map((row) => [row.id, row]));
    for (const entry of SOL_CASES) {
      const row = byId.get(entry.id);
      expect(row, entry.id).toBeDefined();
      for (const op of entry.ops) {
        expect(expectation(row!, op).kind, `${entry.id} ${op}`).toBe("fault");
      }
    }
  });

  describe("on the JVM", () => {
    let runner: RhinoRunner;
    beforeAll(async () => { runner = await RhinoRunner.spawn(); }, 60_000);
    afterAll(async () => { await runner?.close(); }, 30_000);

    const emptyRows = catalog.filter((row) => row.empty !== false);
    it.each(emptyRows)("$id faults or grants every empty read", async (row) => {
      const prefix = row.faultPrefix.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
      const groups: string[] = [];
      for (const op of OPS) {
        const key = faultKey(row, op);
        if (key !== null && !groups.includes(key)) groups.push(key);
      }
      groups.forEach(() => {
        expectOwnedEmulationFailure(new RegExp(`^${prefix}/[\\s\\S]*$`));
      });
      const faultOps = OPS.filter((op) => expectation(row, op).kind === "fault");
      // A caught harness fault still fails the run. The harvest of that run is
      // faultedEffects: the report of every read, including the ones that threw.
      const settled = await runCase(runner, validateCase({
        name: `empty-read ${row.id}`,
        context: row.context,
        script: scriptFor(row),
        given: { engine: row.engine, ...plainGiven(row.given) },
        expect: { allowUndeclared: allow(row.context) },
      })).then(
        (run) => ({ error: null as CaseRunError | null, effects: run.effects, faults: run.response.emulation?.faults ?? [] }),
        (error: unknown) => {
          if (!(error instanceof CaseRunError)) throw error;
          return { error, effects: error.faultedEffects, faults: error.response.emulation?.faults ?? [] };
        },
      );
      if (faultOps.length === 0) {
        expect(settled.error, row.id).toBeNull();
      } else {
        expect(settled.error, row.id).toBeInstanceOf(CaseRunError);
        expect(settled.effects, settled.error?.message).toBeDefined();
      }
      // One cause per refusal. Rhino formats "Cannot find function … in object"
      // by calling String() on the set, so an ungranted empty set records that
      // surface fault again while indexOf or get is refused. The list is longer
      // than the operations; the distinct causes are the decision.
      const causes = [...new Set(settled.faults.map((fault) => `${fault.id}\n${fault.message}`))];
      expect(causes, row.id).toHaveLength(groups.length);
      for (const fault of settled.faults) {
        expect(fault.id.startsWith(`${row.faultPrefix}/`), fault.id).toBe(true);
        expect(fault.message, fault.id).toMatch(/unmeasured|not measured|never produced/);
      }
      if (settled.effects === undefined) throw new Error(`${row.id} produced no harvest`);
      const report = JSON.parse(reportOf(row, settled.effects)) as Record<string, string>;
      for (const op of OPS) {
        const want = expectation(row, op);
        const got = report[op];
        if (want.kind === "value") expect(got, `${op} ${JSON.stringify(report)}`).toBe(`ok:${want.value}`);
        else if (want.kind === "script") {
          expect(got, `${op} ${JSON.stringify(report)}`).toContain(want.needle);
          expect(got?.startsWith("throw:"), op).toBe(true);
        } else {
          expect(got?.startsWith("throw:"), `${op} ${JSON.stringify(report)}`).toBe(true);
          expect(got, `${op} ${JSON.stringify(report)}`).toMatch(/unmeasured|not measured|never produced/);
        }
      }
    }, 60_000);

    const present = catalog.filter((row) => row.empty === false);
    it.each(present)("$id is built non-empty and is still in the catalog", async (row) => {
      const probe = `String(${row.expr}.size())`;
      const source = row.context === "OIDC_CLAIMS"
        ? `var packed = ${probe}; var m = new java.util.LinkedHashMap(); m.put("probe", packed); new org.forgerock.oauth2.core.UserInfoClaims(m, null);`
        : probe;
      if (row.size === undefined) throw new Error(`${row.id} is present but names no size`);
      const run = await runCase(runner, validateCase({
        name: `empty-read present ${row.id}`,
        context: row.context,
        script: source,
        given: { engine: row.engine, ...plainGiven(row.given) },
        expect: { oidc: { userinfo: { probe: row.size } } },
      }));
      expect(run.verdict.pass, run.verdict.summary).toBe(true);
      expect(run.response.emulation?.faults ?? []).toEqual([]);
    }, 60_000);

    const unread: { name: string; context?: ScriptContext; engine?: Engine; script: string; expect: Record<string, JsonValue> }[] = [
      { name: "decision", script: 'outcome="done";', expect: { outcome: "done" } },
      { name: "token", context: "OAUTH2_ACCESS_TOKEN_MODIFICATION", script: ";", expect: {} },
      { name: "oidc", context: "OIDC_CLAIMS", engine: "next-gen", script: "({values:{}});", expect: { oidc: { userinfo: {} } } },
      { name: "oidc", context: "OIDC_CLAIMS", engine: "legacy", script: "new org.forgerock.oauth2.core.UserInfoClaims(new java.util.LinkedHashMap(), null);", expect: { oidc: { userinfo: {} } } },
      { name: "social", context: "SOCIAL_IDP_PROFILE_TRANSFORMATION", script: '({name:"test"});', expect: { socialIdp: { profile: { name: "test" } } } },
      { name: "config", context: "CONFIG_PROVIDER_NODE", script: "config={};", expect: { configProvider: { config: {} } } },
      { name: "validate", context: "OAUTH2_VALIDATE_SCOPE", script: 'function validateAccessTokenScope(){return ["read"];}', expect: { validateScope: { granted: ["read"] } } },
      { name: "may-act", context: "OAUTH2_MAY_ACT", script: ";", expect: {} },
    ];
    const unreadCases = unread.flatMap((row) => (row.engine === undefined ? (["next-gen", "legacy"] as const) : [row.engine]).map((engine) => ({ ...row, engine })));
    it.each(unreadCases)("an unread empty seed stays valid ($name $engine)", async (row) => {
      const run = await runCase(runner, validateCase({
        name: `empty-read unread ${row.name} ${row.engine ?? "next-gen"}`,
        ...(row.context === undefined ? {} : { context: row.context }),
        script: row.script,
        given: { engine: row.engine ?? "next-gen" },
        expect: row.expect,
      }));
      expect(run.verdict.pass, run.verdict.summary).toBe(true);
      expect(run.response.emulation?.faults ?? []).toEqual([]);
    }, 60_000);

    it("an unread library seed stays valid", async () => {
      const run = await runCase(runner, defineCase({
        name: "empty-read unread library",
        context: "LIBRARY",
        script: 'exports.test=function(){return "ok";}',
        given: { library: { export: "test" } },
        expect: { returns: { status: "returned", value: "ok" } },
      }));
      expect(run.verdict.pass, run.verdict.summary).toBe(true);
      expect(run.response.emulation?.faults ?? []).toEqual([]);
    }, 60_000);
  });
});
