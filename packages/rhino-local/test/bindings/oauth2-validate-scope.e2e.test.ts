/**
 * The OAuth2 validate scope context on the real JVM, compared with live
 * captures. surface.script.js ran unchanged as a client's validate scope
 * script on a sandbox tenant (2026-11-10, client_credentials
 * against a stateless client with scopes zzemuc5-a/b/c and default scope
 * zzemuc5-c). Its record is the invalid-scope denial's message, so
 * `surface.<engine>.json` holds it as `probe`, and `noscope.<engine>.json`
 * the same request without a scope parameter. `returns.json` holds one-line
 * scripts that pin what AM does with each kind of result: the granted scope,
 * or the error AM answers. Here the same scripts run in the emulator, and
 * every observable must agree.
 */
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { CaseRunError, runCase } from "../../src/bindings/run.ts";
import { expectHarnessFault } from "../../src/harness/emulation-guard.ts";
import { defineCase } from "../../src/case/validate.ts";
import type { Engine, JsonObject, JsonValue } from "../../src/case/types.ts";
import { packageRoot } from "../../src/paths.ts";
import { RhinoRunner } from "../../src/runner.ts";

const dir = join(packageRoot, "test", "fixtures", "oauth2-validate-scope");
const SURFACE = readFileSync(join(dir, "surface.script.js"), "utf8");

interface Capture {
  http: number;
  probe: { top: JsonObject; fn: JsonObject };
}
const capture = (name: string, engine: Engine) =>
  JSON.parse(readFileSync(join(dir, `${name}.${engine}.json`), "utf8")) as Capture;

interface Returned {
  http: number;
  scope?: string;
  error?: string;
  error_description?: string;
}
const RETURNS = JSON.parse(readFileSync(join(dir, "returns.json"), "utf8")) as Record<
  string,
  { script: string } & Partial<Record<Engine, Returned>>
>;
delete RETURNS._note;

// The sandbox client the probes ran against. Nineteen headers reached the
// script; the probe counts them and reads only content-type.
const headers = () => {
  const out: Record<string, string[]> = { "content-type": ["application/x-www-form-urlencoded"] };
  for (let i = 0; i < 18; i += 1) out[`x-placeholder-${i}`] = ["v"];
  return out;
};
const given = (engine: Engine, requested: string[], loggerName?: string) => ({
  engine,
  realm: "/alpha",
  scriptName: "zzemuC5-vs-ng",
  ...(loggerName === undefined ? {} : { loggerScriptId: /\.([0-9a-f-]{36})\./.exec(loggerName)![1]! }),
  cookieName: "rhino-local-cookie",
  validateScope: { requested, allowed: ["zzemuc5-a", "zzemuc5-b", "zzemuc5-c"], defaults: ["zzemuc5-c"] },
  client: { clientId: "zzemuC5-vs" },
  request: {
    headers: headers(),
    params: {
      client_id: ["zzemuC5-vs"],
      grant_type: ["client_credentials"],
      ...(requested.length === 0 ? {} : { scope: [requested.join(" ")] }),
    },
  },
});

/** Strip what differs by construction: AIC's script-name/line suffix on an InternalError, and Java hash codes. */
const level = (value: JsonValue): JsonValue => {
  if (typeof value === "string") {
    return value.replace(/ \([^()]+#\d+\)$/, "").replace(/@(?:[0-9a-f]{6,8}|rhino-local)\b/g, "@<hash>");
  }
  if (Array.isArray(value)) return value.map(level);
  if (value !== null && typeof value === "object") {
    return Object.fromEntries(Object.entries(value).map(([k, v]) => [k, level(v)]));
  }
  return value;
};

describe("validate scope matches the live capture", () => {
  const engine = "next-gen" as const;
  let runner: RhinoRunner;
  beforeAll(async () => { runner = await RhinoRunner.spawn(); }, 60_000);
  afterAll(async () => { await runner?.close(); }, 30_000);

  const run = (name: string, source: string, requested = ["zzemuc5-a", "zzemuc5-b"], loggerName?: string) =>
    runCase(runner, defineCase({
      name: `${name} ${engine}`,
      context: "OAUTH2_VALIDATE_SCOPE",
      script: source,
      given: given(engine, requested, loggerName),
      expect: { allowUndeclared: { validateScope: true, logs: true } },
    }));

  it.each([
    ["surface", ["zzemuc5-a", "zzemuc5-b"]],
    ["noscope", []],
  ] as const)("%s: the probe's record", async (name, requested) => {
    const live = capture(name, engine);
    expect(live.http).toBe(400);
    const loggerName = (live.probe.fn.loggerName as { ok: { str: string } }).ok.str;
    const { effects } = await run(name, SURFACE, [...requested], loggerName);
    const vs = effects.validateScope!;
    expect(vs.status).toBe(400);
    const record = JSON.parse(vs.invalidScope!) as { top: JsonObject; fn: JsonObject };
    const want = JSON.parse(JSON.stringify(live.probe)) as { top: JsonObject; fn: JsonObject };
    // for...in over a Java object lists its Java members on AIC; the
    // emulator's views list only their own JS members (docs/rhino-local-gaps.md).
    for (const r of [record, want]) {
      delete r.fn.helperMembers;
    }
    expect(level(record as unknown as JsonValue)).toEqual(level(want as unknown as JsonValue));
  }, 60_000);

  const returns = Object.entries(RETURNS).filter(([, byEngine]) => byEngine[engine] !== undefined);
  // Results AM gives that the emulator faults instead: what the call does
  // inside AIC (a missing-overload error the script could catch, or not) was
  // not observed, only the 500 that followed.
  const FAULTED: Record<string, Partial<Record<Engine, string>>> = {
    "invalid-scope-noarg": { "next-gen": "oauth2-validate-scope-unmeasured/scopeValidatorHelper.throwInvalidScope()" },
  };
  it.each(returns)("returns.json %s", async (key, byEngine) => {
    const live = byEngine[engine]!;
    const faulted = FAULTED[key]?.[engine];
    if (faulted !== undefined) expectHarnessFault(faulted);
    const scopeArg = /\[scope=([^\]]*)\]$/.exec(key);
    const requested = scopeArg === null
      ? ["zzemuc5-a", "zzemuc5-b"]
      : scopeArg[1] === "" ? [] : scopeArg[1]!.split(" ");
    const outcome = await run(key, byEngine.script, requested).then((r) => r, (e: unknown) => e);
    if (outcome instanceof CaseRunError) {
      // A script that throws fails the run, as AM fails the request.
      expect(live.http, byEngine.script).toBe(500);
      expect(/throw |InvalidScopeException|throwInvalidScope\(/.test(byEngine.script), byEngine.script).toBe(true);
      return;
    }
    const vs = (outcome as Awaited<ReturnType<typeof run>>).effects.validateScope!;
    expect(vs.status).toBe(live.http);
    if (live.http === 200) {
      expect(vs.refusal).toBeNull();
      expect([...vs.granted!].sort()).toEqual(live.scope!.split(" ").sort());
      return;
    }
    expect(vs.granted).toBeNull();
    expect(vs.refusal).toBe(live.error_description);
    expect(vs.invalidScope).toBe(live.http === 400 ? live.error_description : null);
  }, 60_000);

  const faults: [string, string][] = [
    ["function validateAccessTokenScope() { return true; }", "oauth2-validate-scope-unmeasured/validateAccessTokenScope()"],
    ["function validateAccessTokenScope() { return [true]; }", "oauth2-validate-scope-unmeasured/validateAccessTokenScope()[0]"],
    ["var validateAccessTokenScope = 1;", "oauth2-validate-scope-unmeasured/validateAccessTokenScope"],
  ];
  faults.push(
    ["scopeValidatorHelper.inheritAccessTokenScopesOnRefresh(true); function validateAccessTokenScope() { return []; }",
      "oauth2-validate-scope-unmeasured/scopeValidatorHelper.inheritAccessTokenScopesOnRefresh(boolean)"],
    ["function validateAccessTokenScope() { scopeValidatorHelper.throwInvalidScope({}); }",
      "oauth2-validate-scope-unmeasured/scopeValidatorHelper.throwInvalidScope(object)"],
    ["identity.getUniversalId(); function validateAccessTokenScope() { return []; }",
      "oauth2-validate-scope-unmeasured/identity.getUniversalId()"],
  );
  it.each(faults)("%s is a named fault", async (source, id) => {
    expectHarnessFault(id);
    const error = await run(id, source).then(() => null, (e: unknown) => e);
    expect(error).toBeInstanceOf(CaseRunError);
    const ids = ((error as CaseRunError).response.emulation?.faults ?? []).map((f) => f.id);
    expect(ids).toContain(id);
  }, 60_000);

  it("a mutated requested list is granted with its addition", async () => {
    const { effects } = await run("mutate", "function validateAccessTokenScope() { requestedScopes.add(\"zzemuc5-c\"); return requestedScopes; }");
    expect([...effects.validateScope!.granted!].sort()).toEqual(["zzemuc5-a", "zzemuc5-b", "zzemuc5-c"]);
  }, 60_000);

  const strict = (
    name: string,
    source: string,
    validateScope: { requested?: string[]; allowed?: string[]; defaults?: string[] },
    granted: string[],
  ) => runCase(runner, defineCase({
    name: `${name} ${engine}`,
    context: "OAUTH2_VALIDATE_SCOPE",
    script: source,
    given: { engine, validateScope },
    expect: { validateScope: { granted } },
  }));

  // Sol ctx r5 P2-4. Catching the denial and returning a list is measured.
  // Reading the exception is not, and must fault even when that read is caught.
  const caught = "scopeValidatorHelper.throwInvalidScope(\"denied\")";
  const detailId = "oauth2-validate-scope-unmeasured/scopeValidatorHelper.throwInvalidScope().message";

  it("P2-4 reading a caught denial's message is a named fault", async () => {
    expectHarnessFault(detailId);
    const error = await strict(
      "p2-4-message",
      `function validateAccessTokenScope() { try { ${caught}; } catch (e) { return [e.message]; } }`,
      {},
      ["read"],
    ).then(() => null, (e: unknown) => e);
    expect(error).toBeInstanceOf(CaseRunError);
    const ids = ((error as CaseRunError).response.emulation?.faults ?? []).map((f) => f.id);
    expect(ids).toContain(detailId);
  }, 60_000);

  it("P2-4 reading a caught denial's name is a named fault", async () => {
    const id = detailId.replace(/\.message$/, ".name");
    expectHarnessFault(id);
    const error = await strict(
      "p2-4-name",
      `function validateAccessTokenScope() { try { ${caught}; } catch (e) { return [e.name]; } }`,
      {},
      ["JavaException"],
    ).then(() => null, (e: unknown) => e);
    expect(error).toBeInstanceOf(CaseRunError);
    const ids = ((error as CaseRunError).response.emulation?.faults ?? []).map((f) => f.id);
    expect(ids).toContain(id);
  }, 60_000);

  it("P2-4 String() of a caught denial is a named fault", async () => {
    const id = detailId.replace(/\.message$/, ".toString");
    expectHarnessFault(id);
    const error = await strict(
      "p2-4-string",
      `function validateAccessTokenScope() { try { ${caught}; } catch (e) { return [String(e)]; } }`,
      {},
      ["JavaException"],
    ).then(() => null, (e: unknown) => e);
    expect(error).toBeInstanceOf(CaseRunError);
    const ids = ((error as CaseRunError).response.emulation?.faults ?? []).map((f) => f.id);
    expect(ids).toContain(id);
  }, 60_000);

  it("P2-4 a caught read of the message still faults when the script recovers", async () => {
    expectHarnessFault(detailId);
    const error = await strict(
      "p2-4-recover",
      `function validateAccessTokenScope() { try { ${caught}; } catch (e) { try { return [e.message]; } catch (e2) { return ["read"]; } } }`,
      {},
      ["read"],
    ).then(() => null, (e: unknown) => e);
    expect(error).toBeInstanceOf(CaseRunError);
    const ids = ((error as CaseRunError).response.emulation?.faults ?? []).map((f) => f.id);
    expect(ids).toContain(detailId);
  }, 60_000);

  // Sol ctx r6 P2-1. The round-5 replacement treated the first toString as
  // Rhino's wrap, so a String() before the throw returned the constant.
  // JSON.stringify of the caught object was an empty object. Neither
  // representation was captured.
  it("r6 P2-1 JSON.stringify of a caught denial is a named fault", async () => {
    const id = "oauth2-validate-scope-unmeasured/JSON.stringify(scopeValidatorHelper.throwInvalidScope())";
    expectHarnessFault(id);
    const error = await strict(
      "r6-p2-1-json",
      `function validateAccessTokenScope() { try { ${caught}; } catch (e) { return [JSON.stringify(e)]; } }`,
      {},
      ["{}"],
    ).then(() => null, (e: unknown) => e);
    expect(error).toBeInstanceOf(CaseRunError);
    const ids = ((error as CaseRunError).response.emulation?.faults ?? []).map((f) => f.id);
    expect(ids).toContain(id);
  }, 60_000);

  it("JSON.stringify of a request value is a JSON array", async () => {
    const granted = ['["client_credentials"]'];
    const { effects, verdict } = await strict(
      "p2-6-json",
      "function validateAccessTokenScope() { return [JSON.stringify(requestProperties.requestParams.grant_type)]; }",
      {},
      granted,
    );
    expect(verdict.pass, verdict.summary).toBe(true);
    expect(effects.validateScope!.granted).toEqual(granted);
  }, 60_000);

  it("P2-6 a request value still answers String, length, size and [i]", async () => {
    const { effects } = await strict(
      "p2-6-measured",
      "function validateAccessTokenScope() { var g = requestProperties.requestParams.grant_type; return [String(g) + \"|\" + g[0] + \"|\" + g.length + \"|\" + g.size()]; }",
      {},
      ["[client_credentials]|client_credentials|1|1"],
    );
    expect(effects.validateScope!.granted).toEqual(["[client_credentials]|client_credentials|1|1"]);
  }, 60_000);

  it("P3-1 slice().filter grants the kept scope", async () => {
    const { effects } = await strict(
      "p3-1-slice",
      "function validateAccessTokenScope() { return Array.prototype.slice.call(requestedScopes).filter(function (s) { return s !== \"admin\"; }); }",
      { requested: ["read", "admin"], allowed: ["read", "admin"] },
      ["read"],
    );
    expect(effects.validateScope!.granted).toEqual(["read"]);
  }, 60_000);

  // A list is a real Java list at every size, so an empty one answers as Java does.

  it("JSON of an empty redirect-URI list is []", async () => {
    const granted = ["[]"];
    const { effects, verdict } = await strict(
      "r10-redirect-json",
      "function validateAccessTokenScope() { return [JSON.stringify(clientProperties.redirectUris)]; }",
      {},
      granted,
    );
    expect(verdict.pass, verdict.summary).toBe(true);
    expect(effects.validateScope!.granted).toEqual(granted);
  }, 60_000);

  it("r10 P2-1 empty requested-scope JSON still succeeds", async () => {
    const { effects } = await strict(
      "r10-requested-json",
      "function validateAccessTokenScope() { return [JSON.stringify(requestedScopes)]; }",
      { requested: [] },
      ["[]"],
    );
    expect(effects.validateScope!.granted).toEqual(["[]"]);
  }, 60_000);

  it("concat of an empty requested-scope list is an empty array", async () => {
    const granted = ["0"];
    const { effects, verdict } = await strict(
      "r10-requested-concat",
      "function validateAccessTokenScope() { return [String([].concat(requestedScopes).length)]; }",
      { requested: [] },
      granted,
    );
    expect(verdict.pass, verdict.summary).toBe(true);
    expect(effects.validateScope!.granted).toEqual(granted);
  }, 60_000);

  it("r10 P2-1 an empty redirect-URI list still answers String and length", async () => {
    const { effects } = await strict(
      "r10-redirect-surface",
      "function validateAccessTokenScope() { return [String(clientProperties.redirectUris) + \"|\" + clientProperties.redirectUris.length]; }",
      {},
      ["[]|0"],
    );
    expect(effects.validateScope!.granted).toEqual(["[]|0"]);
  }, 60_000);

  it("r10 P2-1 an empty toArray() result still answers length", async () => {
    const { effects } = await strict(
      "r10-toarray-length",
      "function validateAccessTokenScope() { return [String(requestedScopes.toArray().length)]; }",
      { requested: [] },
      ["0"],
    );
    expect(effects.validateScope!.granted).toEqual(["0"]);
  }, 60_000);

  for (const binding of ["allowedScopes", "availableScopes", "defaultScopes"]) {
    it(`JSON of an empty ${binding} list is []`, async () => {
      const granted = ["[]"];
      const { effects, verdict } = await strict(
        `r11-${binding}-json`,
        `function validateAccessTokenScope() { return [JSON.stringify(${binding})]; }`,
        {},
        granted,
      );
      expect(verdict.pass, verdict.summary).toBe(true);
      expect(effects.validateScope!.granted).toEqual(granted);
    }, 60_000);
  }

  it("an empty allowedScopes list renders [] and has length 0", async () => {
    const granted = ["[]|0"];
    const { effects, verdict } = await strict(
      "r11-allowed-surface",
      "function validateAccessTokenScope() { return [String(allowedScopes) + \"|\" + allowedScopes.length]; }",
      {},
      granted,
    );
    expect(verdict.pass, verdict.summary).toBe(true);
    expect(effects.validateScope!.granted).toEqual(granted);
  }, 60_000);

  it("an empty allowedScopes.toArray() has length 0", async () => {
    const granted = ["0"];
    const { effects, verdict } = await strict(
      "r11-allowed-toarray",
      "function validateAccessTokenScope() { return [String(allowedScopes.toArray().length)]; }",
      {},
      granted,
    );
    expect(verdict.pass, verdict.summary).toBe(true);
    expect(effects.validateScope!.granted).toEqual(granted);
  }, 60_000);

  const r12Method = (binding: string, op: string) => {
    if (op === "forEach") {
      return "function validateAccessTokenScope() { var n = 0; " + binding +
        ".forEach(function () { n += 1; }); return [String(n)]; }";
    }
    const call = op === "isEmpty" ? "isEmpty()" : op + "(\"read\")";
    return "function validateAccessTokenScope() { return [String(" + binding + "." + call + ")]; }";
  };
  const r12Methods = [
    ["contains", "false"],
    ["indexOf", "-1"],
    ["isEmpty", "true"],
    ["forEach", "0"],
  ] as const;

  it("r12 P2-1 empty requestedScopes JSON still succeeds", async () => {
    const { effects, verdict } = await strict(
      "r12-requested-json",
      "function validateAccessTokenScope() { return [JSON.stringify(requestedScopes)]; }",
      { requested: [] },
      ["[]"],
    );
    expect(verdict.pass, verdict.summary).toBe(true);
    expect(effects.validateScope!.granted).toEqual(["[]"]);
  }, 60_000);

  for (const [op, value] of r12Methods) {
    it(`${op} of an empty allowedScopes list answers as Java does`, async () => {
      const granted = [value];
      const { effects, verdict } = await strict(
        `r12-allowed-${op}`,
        r12Method("allowedScopes", op),
        {},
        granted,
      );
      expect(verdict.pass, verdict.summary).toBe(true);
      expect(effects.validateScope!.granted).toEqual(granted);
    }, 60_000);
  }

  for (const [op, value] of r12Methods) {
    it(`r12 P2-1 ${op} of an empty requestedScopes list still succeeds`, async () => {
      const { effects, verdict } = await strict(
        `r12-requested-${op}`,
        r12Method("requestedScopes", op),
        { requested: [] },
        [value],
      );
      expect(verdict.pass, verdict.summary).toBe(true);
      expect(effects.validateScope!.granted).toEqual([value]);
    }, 60_000);
  }

  it("r12 P2-1 an empty requestedScopes list still grows through add()", async () => {
    const { effects, verdict } = await strict(
      "r12-requested-add",
      "function validateAccessTokenScope() { requestedScopes.add(\"read\"); " +
        "return [String(requestedScopes) + \"|\" + requestedScopes.length + \"|\" + JSON.stringify(requestedScopes)]; }",
      { requested: [] },
      ["[read]|1|[\"read\"]"],
    );
    expect(verdict.pass, verdict.summary).toBe(true);
    expect(effects.validateScope!.granted).toEqual(["[read]|1|[\"read\"]"]);
  }, 60_000);
});
