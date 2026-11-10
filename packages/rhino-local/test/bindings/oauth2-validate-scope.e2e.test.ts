/**
 * The OAuth2 validate scope context on the real JVM, compared with live
 * captures. surface.script.js ran unchanged as a client's validate scope
 * script on a sandbox tenant (2026-11-10, both engines, client_credentials
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
  scriptName: engine === "next-gen" ? "zzemuC5-vs-ng" : "zzemuC5-vs-legacy",
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
    return value.replace(/ \([^()\s]+#\d+\)$/, "").replace(/@(?:[0-9a-f]{6,8}|rhino-local)\b/g, "@<hash>");
  }
  if (Array.isArray(value)) return value.map(level);
  if (value !== null && typeof value === "object") {
    return Object.fromEntries(Object.entries(value).map(([k, v]) => [k, level(v)]));
  }
  return value;
};

describe.each(["next-gen", "legacy"] as const)("validate scope on %s matches the live capture", (engine) => {
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
      if (engine === "legacy") {
        delete (r.fn.requested as JsonObject).forIn;
        delete (r.fn.allowedList as JsonObject).forIn;
        // String(s.length) of a guarded java.lang.String is a JS function's
        // source, not Rhino's rendering of the Java method (gaps).
        delete ((r.fn.loggerName as JsonObject).ok as JsonObject).len;
      }
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
    ["String(defaultScopes.add); defaultScopes.add(\"x\"); function validateAccessTokenScope() { return []; }",
      "oauth2-validate-scope-unmeasured/defaultScopes.add()"],
  ];
  if (engine === "next-gen") {
    faults.push(
      ["scopeValidatorHelper.inheritAccessTokenScopesOnRefresh(true); function validateAccessTokenScope() { return []; }",
        "oauth2-validate-scope-unmeasured/scopeValidatorHelper.inheritAccessTokenScopesOnRefresh(boolean)"],
      ["function validateAccessTokenScope() { scopeValidatorHelper.throwInvalidScope({}); }",
        "oauth2-validate-scope-unmeasured/scopeValidatorHelper.throwInvalidScope(object)"],
      ["identity.getUniversalId(); function validateAccessTokenScope() { return []; }",
        "oauth2-validate-scope-unmeasured/identity.getUniversalId()"],
    );
  } else {
    faults.push(
      ["requestedScopes.iterator().hasNext(); function validateAccessTokenScope() { return []; }",
        "oauth2-validate-scope-unmeasured/requestedScopes.iterator().hasNext()"],
    );
  }
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
});
