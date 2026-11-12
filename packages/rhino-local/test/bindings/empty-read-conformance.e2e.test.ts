/**
 * Every collection a script context builds, every empty read (Sol ctx r13
 * P2-1, r14 P2-1 and P2-2).
 *
 * The inventory is registration, not a list anyone keeps in step with the
 * code. Each collection constructor names its binding, and
 * `__rhinoLocalCollectionRegister` (runtime.cjs) resolves it to a row of
 * `collection-catalog.cjs` and records the id in `__rhinoLocal.collections`.
 * A constructor that names no binding throws, and one whose binding has no
 * row is a tallied fault when it is built, so a new collection either fails
 * every case that reaches it or has a row. The matrix below is the catalog;
 * the registration test checks it against what the runtime actually built,
 * on each context's default seed and on a seed where every collection is
 * empty. On a size-zero seed each operation is a named unmeasured fault, or
 * a grant that names the capture that measured that operation.
 */
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import vm from "node:vm";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { CaseRunError, runCase } from "../../src/bindings/run.ts";
import type { CompleteRecordedEffects, Engine, JsonValue, ScriptContext } from "../../src/case/types.ts";
import { defineCase, validateCase } from "../../src/case/validate.ts";
import { expectOwnedEmulationFailure } from "../../src/emulation.ts";
import { packageRoot, repoRoot } from "../../src/paths.ts";
import { RhinoRunner } from "../../src/runner.ts";

/** `add` is last: it grows the collection, and every read before it is of the empty one. */
const OPS = [
  "size", "length", "string", "isEmpty", "contains", "indexOf", "forEach",
  "toArray", "toArraySize", "toArrayLength", "toArrayJson", "toArrayIndex", "toArrayIndex8",
  "json", "concat", "get", "index", "index8", "index1000", "forInKeys", "forInValues", "iterator", "add",
] as const;
type Op = (typeof OPS)[number];

const GRANT_OPS = [
  "surface", "length", "json", "concat", "contains", "indexOf", "isEmpty", "forEach", "toArray", "add",
  "size", "get", "index", "forIn", "iterator", "lengthAbsent", "toArrayLength", "toArraySize",
  "toArrayJson", "toArrayIndex",
] as const;
type Grant = Partial<Record<(typeof GRANT_OPS)[number], string>>;

interface Row {
  id: string;
  binding: string;
  context: ScriptContext;
  engine: Engine;
  kind: "list";
  expr: string;
  faultPrefix: string;
  toArrayCall: "guarded" | "stub";
  grant: Grant | null;
  given: Record<string, JsonValue>;
  iteratorShutter?: string;
  empty?: boolean;
  size?: string;
}

const catalogPath = join(packageRoot, "src", "bindings", "rhino", "collection-catalog.cjs");

function loadCatalog(): Row[] {
  const sandbox: { __rhinoLocalCollectionCatalog?: Row[] } = {};
  vm.createContext(sandbox);
  vm.runInContext(readFileSync(catalogPath, "utf8"), sandbox, { filename: catalogPath });
  const rows = sandbox.__rhinoLocalCollectionCatalog;
  if (!Array.isArray(rows) || rows.length === 0) {
    throw new Error("collection catalog did not load");
  }
  // vm-realm objects; `defineCase` rejects those, and a JSON clone is plain.
  return JSON.parse(JSON.stringify(rows)) as Row[];
}

const catalog = loadCatalog();

const CONTEXT_KEYS: Record<string, ScriptContext> = {
  validate: "OAUTH2_VALIDATE_SCOPE",
  token: "OAUTH2_ACCESS_TOKEN_MODIFICATION",
  oidc: "OIDC_CLAIMS",
  mayact: "OAUTH2_MAY_ACT",
  social: "SOCIAL_IDP_PROFILE_TRANSFORMATION",
  config: "CONFIG_PROVIDER_NODE",
  library: "LIBRARY",
};

type Expectation =
  | { kind: "value"; value: string }
  | { kind: "fault" }
  | { kind: "script"; needle: string };

function allows(row: Row, route: string): boolean {
  const grant = row.grant;
  if (grant === null) return false;
  if (route === "length") return grant.length !== undefined || (grant.surface !== undefined && row.kind === "list");
  return grant[route as keyof Grant] !== undefined;
}

/** The toArray() call answered (it did not fault), so its child's reads are the child grant. */
function toArrayAnswers(row: Row): boolean {
  return row.toArrayCall === "guarded";
}

function expectation(row: Row, op: Op): Expectation {
  const grant = row.grant;
  if (op === "length" && grant?.lengthAbsent !== undefined) return { kind: "value", value: "undefined" };
  if (op === "toArray") return toArrayAnswers(row) ? { kind: "value", value: "object" } : { kind: "fault" };
  if (op.startsWith("toArray")) {
    if (!toArrayAnswers(row)) return { kind: "fault" };
    const child: Record<string, [keyof Grant, string]> = {
      toArraySize: ["toArraySize", "0"],
      toArrayLength: ["toArrayLength", "0"],
      toArrayJson: ["toArrayJson", "[]"],
      toArrayIndex: ["toArrayIndex", "undefined"],
      toArrayIndex8: ["toArrayIndex", "undefined"],
    };
    const [field, value] = child[op]!;
    return grant?.[field] !== undefined ? { kind: "value", value } : { kind: "fault" };
  }
  if (op === "iterator") {
    if (row.iteratorShutter !== undefined) return { kind: "script", needle: row.iteratorShutter };
    return allows(row, "iterator") ? { kind: "value", value: "object" } : { kind: "fault" };
  }
  if (op === "index" || op === "index8" || op === "index1000") {
    return allows(row, "index") ? { kind: "value", value: "undefined" } : { kind: "fault" };
  }
  if (op === "forInKeys") return allows(row, "forIn") ? { kind: "value", value: "0" } : { kind: "fault" };
  if (op === "forInValues") return allows(row, "forIn") ? { kind: "value", value: "" } : { kind: "fault" };
  // openidm's get(i) is its measured out-of-range InternalError.
  if (op === "get" && row.binding === "openidm" && allows(row, "get")) {
    return { kind: "script", needle: "Index 0 out of bounds for length 0" };
  }
  if (op === "string") return allows(row, "surface") ? { kind: "value", value: "[]" } : { kind: "fault" };
  if (op === "length") return allows(row, "length") ? { kind: "value", value: "0" } : { kind: "fault" };
  const values: Partial<Record<Op, string>> = {
    size: "0",
    isEmpty: "true",
    contains: "false",
    indexOf: "-1",
    forEach: "0",
    json: "[]",
    concat: "0",
    get: "undefined",
    add: "true",
  };
  if (values[op] !== undefined && allows(row, op)) return { kind: "value", value: values[op]! };
  return { kind: "fault" };
}

/**
 * One harness cause per distinct refusal. A child read never runs when the
 * toArray() call itself is refused, a set's indexOf and get surface the
 * String() fault (Rhino formats "Cannot find function … in object" by calling
 * it), and both for...in loops hit the one enumeration fault.
 */
function faultKey(row: Row, op: Op): string | null {
  if (expectation(row, op).kind !== "fault") return null;
  if (op.startsWith("toArray") && op !== "toArray" && expectation(row, "toArray").kind === "fault") return "toArray";
  if (op === "forInValues") return "forInKeys";
  return op;
}

/** Sol ctx r13's green cases (the next-gen ones). Each must be a fault, not a grant. */
const SOL_R13: { id: string; ops: Op[] }[] = [
  { id: "validate.next.allowedScopes", ops: ["size"] },
  { id: "validate.next.availableScopes", ops: ["size"] },
  { id: "validate.next.defaultScopes", ops: ["size"] },
  { id: "oidc.next.scopes", ops: ["size"] },
  { id: "oidc.next.scopes", ops: ["toArraySize"] },
  { id: "mayact.next.scopes", ops: ["size"] },
  { id: "validate.next.allowedScopes", ops: ["toArraySize"] },
  { id: "social.next.rawProfile.keys", ops: ["size"] },
];

/**
 * Sol ctx r14's green reproductions on 8043893c. Each is a fault now, and
 * the JVM matrix runs every one of them.
 */
const SOL_R14: { id: string; ops: Op[] }[] = [
  { id: "token.next.scopes", ops: ["forInKeys"] },
  { id: "social.next.rawProfile.keys", ops: ["forInKeys"] },
  { id: "validate.next.allowedScopes", ops: ["forInKeys"] },
  { id: "validate.next.availableScopes", ops: ["forInKeys"] },
  { id: "validate.next.defaultScopes", ops: ["forInKeys"] },
  // The capture lists Java members (getClass, …); the view had invented .
  { id: "token.next.scopes", ops: ["index8"] },
  { id: "oidc.next.scopes", ops: ["toArrayIndex8"] },
  // Granted without a capture that calls it.
  // The request kind's allowEmptySize answered 0 with no capture.
  { id: "social.next.requestParameters", ops: ["size"] },
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
    `read("toArrayIndex", function () { return typeof ${expr}.toArray()[0]; });`,
    `read("toArrayIndex8", function () { return typeof ${expr}.toArray()[8]; });`,
    `read("json", function () { return JSON.stringify(${expr}); });`,
    `read("concat", function () { return [].concat(${expr}).length; });`,
    `read("get", function () { return ${expr}.get(0); });`,
    `read("index", function () { return typeof ${expr}[0]; });`,
    `read("index8", function () { return typeof ${expr}[8]; });`,
    `read("index1000", function () { return typeof ${expr}[1000]; });`,
    `read("forInKeys", function () { var n = 0; var v = ${expr}; for (var k in v) { n += 1; } return n; });`,
    `read("forInValues", function () { var s = ""; var v = ${expr}; for (var k in v) { s += String(v[k]); } return s; });`,
    `read("iterator", function () { return typeof ${expr}.iterator(); });`,
    `read("add", function () { return ${expr}.add("x"); });`,
    "report.registered = (__rhinoLocal.collections || []).join(\",\");",
  ].join("\n");
}

/** The script that runs `body` (which fills `report`) and hands it back through the context's own output. */
function wrap(context: ScriptContext, body: string): string {
  const packed = `${body}\nvar packed = JSON.stringify(report);`;
  switch (context) {
    case "OAUTH2_VALIDATE_SCOPE":
      return `function validateAccessTokenScope() {\n${body}\nreturn [JSON.stringify(report)];\n}`;
    case "OAUTH2_ACCESS_TOKEN_MODIFICATION":
      return `${packed}\naccessToken.setField("probe", packed);`;
    case "OIDC_CLAIMS":
      return `${packed}\n({values:{probe:packed}});`;
    case "OAUTH2_MAY_ACT":
      return `${packed}\ntoken.setMayAct({probe:packed});`;
    case "SOCIAL_IDP_PROFILE_TRANSFORMATION":
      return `${packed}\n({probe:packed});`;
    case "CONFIG_PROVIDER_NODE":
      return `${packed}\nconfig = {probe:packed};`;
    case "LIBRARY":
      return `exports.test = function () {\n${body}\nreturn JSON.stringify(report);\n};`;
    default:
      throw new Error(`no script wrapper for ${context}`);
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
    case "LIBRARY": return { returns: true };
    default: return {};
  }
}

function reportOf(context: ScriptContext, effects: CompleteRecordedEffects): string {
  switch (context) {
    case "OAUTH2_VALIDATE_SCOPE": return effects.validateScope?.granted?.[0] ?? "";
    case "OAUTH2_ACCESS_TOKEN_MODIFICATION": return String(effects.accessToken?.final.fields.probe ?? "");
    case "OIDC_CLAIMS": return String(effects.oidc?.userinfo.probe ?? "");
    case "OAUTH2_MAY_ACT": {
      const claim = effects.mayAct?.mayAct;
      return claim !== null && typeof claim === "object" && !Array.isArray(claim) ? String(claim.probe ?? "") : "";
    }
    case "SOCIAL_IDP_PROFILE_TRANSFORMATION": return String(effects.socialIdp?.profile?.probe ?? "");
    case "CONFIG_PROVIDER_NODE": return String(effects.configProvider?.config?.probe ?? "");
    case "LIBRARY": {
      const returned = effects.returns as { status?: string; value?: JsonValue } | undefined;
      return returned?.status === "returned" ? String(returned.value ?? "") : "";
    }
    default: return "";
  }
}

interface Settled {
  error: CaseRunError | null;
  report: Record<string, string>;
  faults: { id: string; message: string }[];
}

async function settle(runner: RhinoRunner, name: string, context: ScriptContext, engine: Engine,
  script: string, given: Record<string, JsonValue>): Promise<Settled> {
  const kase = context === "LIBRARY"
    ? defineCase({
      name,
      context,
      script,
      given: { library: { export: "test" }, ...given },
      expect: { allowUndeclared: allow(context) },
    } as Parameters<typeof defineCase>[0])
    : validateCase({
      name,
      context,
      script,
      given: { engine, ...given },
      expect: { allowUndeclared: allow(context) },
    } as Parameters<typeof validateCase>[0]);
  const outcome = await runCase(runner, kase).then(
    (run) => ({ error: null as CaseRunError | null, effects: run.effects as CompleteRecordedEffects | undefined, faults: run.response.emulation?.faults ?? [] }),
    (error: unknown) => {
      if (!(error instanceof CaseRunError)) throw error;
      return { error, effects: error.faultedEffects, faults: error.response.emulation?.faults ?? [] };
    },
  );
  if (outcome.effects === undefined) throw new Error(`${name} produced no harvest: ${outcome.error?.message}`);
  const text = reportOf(context, outcome.effects);
  if (text === "") throw new Error(`${name} reported nothing: ${outcome.error?.message}`);
  return { error: outcome.error, report: JSON.parse(text) as Record<string, string>, faults: outcome.faults };
}

function isPlainSeed(value: JsonValue | undefined): value is Record<string, JsonValue> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function mergeSeedInto(target: Record<string, JsonValue>, source: Record<string, JsonValue>): Record<string, JsonValue> {
  for (const [key, value] of Object.entries(source)) {
    const current = target[key];
    if (isPlainSeed(value)) {
      target[key] = mergeSeedInto(isPlainSeed(current) ? { ...current } : {}, value);
    } else {
      target[key] = value;
    }
  }
  return target;
}

/**
 * Every row's seed at once (Sol ctx r15 P3-1). Plain objects merge
 * recursively, so two rows seeding different keys of `client` or `request`
 * both keep theirs; a shallow `Object.assign` kept only the last row's object.
 */
function mergeSeeds(seeds: Record<string, JsonValue>[]): Record<string, JsonValue> {
  return seeds.reduce<Record<string, JsonValue>>((merged, seed) => mergeSeedInto(merged, seed), {});
}

/** `seed` holds every leaf of `part`. */
function seedHolds(seed: Record<string, JsonValue>, part: Record<string, JsonValue>): boolean {
  return Object.entries(part).every(([key, value]) => {
    const held = seed[key];
    return isPlainSeed(value)
      ? isPlainSeed(held) && seedHolds(held, value)
      : JSON.stringify(held) === JSON.stringify(value);
  });
}

/** A capture citation is "<path> <key>", several joined by "; ". */
function citedPaths(citation: string): string[] {
  return citation.split("; ").map((part) => part.split(" ")[0]!);
}

const pairs = [...new Set(catalog.map((row) => `${row.context}\n${row.engine}`))].map((key) => {
  const [context, engine] = key.split("\n") as [ScriptContext, Engine];
  return { context, engine };
});

describe("empty-read conformance (Sol ctx r13 P2-1, r14 P2-1 and P2-2)", () => {
  it("names each row by its context, engine and binding, and cites a capture that exists for every grant", () => {
    const ids = catalog.map((row) => row.id);
    expect(new Set(ids).size).toBe(ids.length);
    for (const row of catalog) {
      const key = row.id.split(".")[0]!;
      expect(CONTEXT_KEYS[key], row.id).toBe(row.context);
      expect(row.id, row.id).toBe(`${key}.next.${row.binding}`);
      for (const [op, citation] of Object.entries(row.grant ?? {})) {
        expect(GRANT_OPS as readonly string[], `${row.id} ${op}`).toContain(op);
        expect(typeof citation, `${row.id} ${op}`).toBe("string");
        for (const path of citedPaths(citation)) {
          expect(existsSync(join(packageRoot, path)) || existsSync(join(repoRoot, path)), `${row.id} ${op}: ${path}`).toBe(true);
        }
      }
    }
  });

  it("records Sol's round-13 and round-14 cases as faults", () => {
    expect(SOL_R13).toHaveLength(8);
    const byId = new Map(catalog.map((row) => [row.id, row]));
    for (const entry of [...SOL_R13, ...SOL_R14]) {
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

    // The record: what the runtime built on the default seed, and on one
    // seed where every collection of the context is empty while the script
    // reaches each binding. Every id built has a row, and every row is built.
    it.each(pairs)("registers exactly the catalog's collections ($context $engine)", async ({ context, engine }) => {
      const rows = catalog.filter((row) => row.context === context && row.engine === engine);
      const emptySeed = mergeSeeds(rows.map((row) => row.given));
      for (const row of rows) expect(seedHolds(emptySeed, row.given), `${row.id} seeded`).toBe(true);
      const touch = rows.map((row) => `try { ${row.expr}; } catch (e) {}`).join("\n");
      const recorded = "report.registered = __rhinoLocal.collections.join(\",\");";
      const unread = await settle(runner, `registration default ${context} ${engine}`, context, engine,
        wrap(context, `var report = {};\n${recorded}`), {});
      expect(unread.error, unread.error?.message).toBeNull();
      const reached = await settle(runner, `registration empty ${context} ${engine}`, context, engine,
        wrap(context, `var report = {};\n${touch}\n${recorded}`), emptySeed);
      expect(reached.error, reached.error?.message).toBeNull();
      const built = new Set([unread.report.registered, reached.report.registered]
        .flatMap((ids) => (ids ? ids.split(",") : [])));
      const known = new Set(rows.map((row) => row.id));
      expect([...built].filter((id) => !known.has(id)), "built with no row").toEqual([]);
      expect([...known].filter((id) => !built.has(id)), "row never built").toEqual([]);
    }, 120_000);

    // Sol ctx r14 P2-2's scenario: a new constructor inserted into a binding.
    // One that names no binding throws at construction (a plain Error, so the
    // script can catch it); one whose binding has no row is a tallied fault.
    it("refuses a collection that names no binding, and faults one whose binding has no row", async () => {
      const noBinding = await settle(runner, "registration no binding", "OAUTH2_ACCESS_TOKEN_MODIFICATION", "next-gen",
        wrap("OAUTH2_ACCESS_TOKEN_MODIFICATION",
          'var report = {};\ntry { __rhinoLocalCtxList([], "futureBinding"); report.built = "yes"; } catch (e) { report.built = String(e); }'),
        {});
      expect(noBinding.error, noBinding.error?.message).toBeNull();
      expect(noBinding.report.built).toContain("must name its catalog binding");
      expectOwnedEmulationFailure(/^rhino-local-collection-unregistered\/token\.next\.futureBinding: [\s\S]*$/);
      const noRow = await settle(runner, "registration no row", "OAUTH2_ACCESS_TOKEN_MODIFICATION", "next-gen",
        wrap("OAUTH2_ACCESS_TOKEN_MODIFICATION",
          'var report = {};\ntry { __rhinoLocalCtxList([], "futureBinding", null, "futureBinding"); report.built = "yes"; } catch (e) { report.built = String(e); }'),
        {});
      expect(noRow.error).toBeInstanceOf(CaseRunError);
      expect(noRow.report.built).toContain("has no row in collection-catalog.cjs");
      expect(noRow.faults.map((fault) => fault.id)).toEqual(["rhino-local-collection-unregistered/token.next.futureBinding"]);
    }, 60_000);

    const emptyRows = catalog.filter((row) => row.empty !== false);
    it.each(emptyRows)("$id faults or grants every empty read", async (row) => {
      const prefix = row.faultPrefix.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
      const groups: string[] = [];
      for (const op of OPS) {
        const key = faultKey(row, op);
        if (key !== null && !groups.includes(key)) groups.push(key);
      }
      groups.forEach(() => {
        // A cause is "<id>: <message>". Context faults name the operation in
        // the id ("<prefix>/<what>"); the shared list kinds' ids are the bare prefix.
        expectOwnedEmulationFailure(new RegExp(`^${prefix}(/|: )[\\s\\S]*$`));
      });
      // A caught harness fault still fails the run. The harvest of that run is
      // faultedEffects: the report of every read, including the ones that threw.
      const settled = await settle(runner, `empty-read ${row.id}`, row.context, row.engine,
        wrap(row.context, reads(row.expr)), row.given);
      const report = settled.report;
      // Every operation first, softly, so a regression names each read it changed.
      for (const op of OPS) {
        const want = expectation(row, op);
        const got = report[op];
        const where = `${row.id} ${op}: ${got}`;
        if (want.kind === "value") expect.soft(got, where).toBe(`ok:${want.value}`);
        else if (want.kind === "script") {
          expect.soft(got?.startsWith("throw:") && got.includes(want.needle), where).toBe(true);
        } else {
          expect.soft(got?.startsWith("throw:") && /unmeasured|not measured|never produced/.test(got), where).toBe(true);
        }
      }
      if (groups.length === 0) expect(settled.error, row.id).toBeNull();
      else expect(settled.error, row.id).toBeInstanceOf(CaseRunError);
      expect(report.registered?.split(","), `${row.id} registered`).toContain(row.id);
      // One cause per refusal. Rhino formats "Cannot find function … in
      // object" by calling String() on the set, which records that fault
      // again; the distinct causes are the decision.
      const causes = [...new Set(settled.faults.map((fault) => `${fault.id}\n${fault.message}`))];
      expect(causes, `${row.id} ${JSON.stringify(report)}`).toHaveLength(groups.length);
      for (const fault of settled.faults) {
        expect(fault.id === row.faultPrefix || fault.id.startsWith(`${row.faultPrefix}/`), fault.id).toBe(true);
        expect(fault.message, fault.id).toMatch(/unmeasured|not measured|never produced/);
      }
    }, 60_000);

    const present = catalog.filter((row) => row.empty === false);
    it.each(present)("$id is built non-empty and is still in the catalog", async (row) => {
      if (row.size === undefined) throw new Error(`${row.id} is present but names no size`);
      const settled = await settle(runner, `empty-read present ${row.id}`, row.context, row.engine,
        wrap(row.context,
          `var report = {};\nreport.size = String(${row.expr}.size());\nreport.registered = __rhinoLocal.collections.join(",");`),
        row.given);
      expect(settled.error, settled.error?.message).toBeNull();
      expect(settled.faults).toEqual([]);
      expect(settled.report.size).toBe(row.size);
      expect(settled.report.registered?.split(",")).toContain(row.id);
    }, 60_000);

    const unread: { name: string; context?: ScriptContext; engine?: Engine; script: string; expect: Record<string, JsonValue> }[] = [
      { name: "decision", script: 'action.goTo("done");', expect: { outcome: "done" } },
      { name: "token", context: "OAUTH2_ACCESS_TOKEN_MODIFICATION", script: ";", expect: {} },
      { name: "oidc", context: "OIDC_CLAIMS", engine: "next-gen", script: "({values:{}});", expect: { oidc: { userinfo: {} } } },
      { name: "social", context: "SOCIAL_IDP_PROFILE_TRANSFORMATION", script: '({name:"test"});', expect: { socialIdp: { profile: { name: "test" } } } },
      { name: "config", context: "CONFIG_PROVIDER_NODE", script: "config={};", expect: { configProvider: { config: {} } } },
      { name: "validate", context: "OAUTH2_VALIDATE_SCOPE", script: 'function validateAccessTokenScope(){return ["read"];}', expect: { validateScope: { granted: ["read"] } } },
      { name: "may-act", context: "OAUTH2_MAY_ACT", script: ";", expect: {} },
    ];
    const unreadCases = unread.flatMap((row) => (row.engine === undefined ? (["next-gen"] as const) : [row.engine]).map((engine) => ({ ...row, engine })));
    it.each(unreadCases)("an unread empty seed stays valid ($name $engine)", async (row) => {
      // Every collection the catalog seeds for this context, all empty, none read.
      const given = row.context === undefined
        ? {}
        : mergeSeeds(catalog.filter((r) => r.context === row.context && r.engine === row.engine).map((r) => r.given));
      const run = await runCase(runner, validateCase({
        name: `empty-read unread ${row.name} ${row.engine ?? "next-gen"}`,
        ...(row.context === undefined ? {} : { context: row.context }),
        script: row.script,
        given: { engine: row.engine ?? "next-gen", ...given },
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
        given: { library: { export: "test" }, managed: { "managed/alpha_user": [] } },
        expect: { returns: { status: "returned", value: "ok" } },
      }));
      expect(run.verdict.pass, run.verdict.summary).toBe(true);
      expect(run.response.emulation?.faults ?? []).toEqual([]);
    }, 60_000);
  });
});
