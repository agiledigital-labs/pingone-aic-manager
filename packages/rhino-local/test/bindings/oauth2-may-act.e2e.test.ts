/**
 * The OAuth2 may-act context on the real JVM, compared with live captures.
 * surface.script.js ran unchanged as a client's may-act script on a sandbox
 * tenant (2026-11-10, both engines, client_credentials). Its record leaves
 * as the may_act claim's probe member, so surface.<engine>.json holds that
 * probe already parsed, plus the claim with the probe removed.
 * returns.json holds one script per engine and the claims AM stamped, or
 * the error AM answered. Here the same scripts run in the emulator, and
 * every observable must agree.
 */
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { CaseRunError, runCase } from "../../src/bindings/run.ts";
import { expectHarnessFault } from "../../src/harness/emulation-guard.ts";
import { defineCase } from "../../src/case/validate.ts";
import type { Engine, JsonObject, JsonValue, MayActEffect } from "../../src/case/types.ts";
import { packageRoot } from "../../src/paths.ts";
import { RhinoRunner } from "../../src/runner.ts";

const dir = join(packageRoot, "test", "fixtures", "oauth2-may-act");
const SURFACE = readFileSync(join(dir, "surface.script.js"), "utf8");

interface SurfaceCapture {
  http: number;
  act: JsonValue;
  mayActWithoutProbe: JsonObject;
  probe: JsonObject;
}
const surface = (engine: Engine) =>
  JSON.parse(readFileSync(join(dir, `surface.${engine}.json`), "utf8")) as SurfaceCapture;

interface Returned {
  http: number;
  script: string;
  may_act?: JsonValue;
  act?: JsonValue;
  scope?: string;
  error_description?: string;
}
const RETURNS = JSON.parse(readFileSync(join(dir, "returns.json"), "utf8")) as Record<
  string,
  { _note?: string } & Partial<Record<Engine, Returned>>
>;
delete RETURNS._note;

const CLIENT = "zzemuC6-ma";
const SCOPE = "zzemuc6-a";

const given = (engine: Engine, loggerName: string) => ({
  engine,
  realm: "/alpha",
  scriptName: engine === "next-gen" ? "zzemuC6-ma-ng" : "zzemuC6-ma-legacy",
  loggerScriptId: /\.([0-9a-f-]{36})\./.exec(loggerName)![1]!,
  cookieName: "rhino-local-cookie",
  scopes: [SCOPE],
  client: { clientId: CLIENT },
  request: {
    headers: { "content-type": ["application/x-www-form-urlencoded"] },
    params: { client_id: [CLIENT], grant_type: ["client_credentials"] },
  },
});

/**
 * Strip what differs by construction: AIC's script location (including the
 * nested `(eval)` form the may-act probe records) and Java hash codes.
 */
const level = (value: JsonValue): JsonValue => {
  if (typeof value === "string") {
    return value
      .replace(/ \((?:[^()]|\([^()]*\))*\)$/g, "")
      .replace(/@(?:[0-9a-f]{6,8}|rhino-local)\b/g, "@<hash>");
  }
  if (Array.isArray(value)) return value.map(level);
  if (value !== null && typeof value === "object") {
    return Object.fromEntries(Object.entries(value).map(([k, v]) => [k, level(v)]));
  }
  return value;
};

/** `<absent>` in the captures is a missing claim, which is not JSON null. */
const claim = (present: boolean, value: JsonValue | null): JsonValue => (present ? value : "<absent>");

describe.each(["next-gen", "legacy"] as const)("may act on %s matches the live capture", (engine) => {
  let runner: RhinoRunner;
  beforeAll(async () => { runner = await RhinoRunner.spawn(); }, 60_000);
  afterAll(async () => { await runner?.close(); }, 30_000);

  const run = (name: string, source: string, loggerName: string, issued = true) =>
    runCase(runner, defineCase({
      name: `${name} ${engine}`,
      context: "OAUTH2_MAY_ACT",
      script: source,
      given: given(engine, loggerName),
      expect: {
        allowUndeclared: { mayAct: true, logs: true },
        ...(issued ? {} : { mayAct: { issued: false } }),
      },
    }));

  it("surface: the probe's record", async () => {
    const live = surface(engine);
    expect(live.http).toBe(200);
    const loggerName = (live.probe.loggerName as { ok: { str: string } }).ok.str;
    const { effects } = await run("surface", SURFACE, loggerName);
    const ma = effects.mayAct!;
    expect(ma.status).toBe(200);
    const stamped = ma.mayAct as JsonObject;
    const probe = JSON.parse(stamped.probe as string) as JsonObject;
    const rest = { ...stamped };
    delete rest.probe;
    expect(rest).toEqual(live.mayActWithoutProbe);
    expect(claim(ma.actPresent, ma.act)).toEqual(live.act);
    const want = JSON.parse(JSON.stringify(live.probe)) as JsonObject;
    // for...in over the token lists Java members on next-gen and throws the
    // ResourceOwner shutter on legacy. Neither is reproduced (gaps).
    delete probe.tokenMembers;
    delete want.tokenMembers;
    if (engine === "legacy") {
      // String(s.length) of a guarded java.lang.String is a JS function's
      // source, not Rhino's rendering of the Java method (gaps).
      for (const key of ["idName", "loggerName", "rpRealm"]) {
        delete ((probe[key] as JsonObject).ok as JsonObject).len;
        delete ((want[key] as JsonObject).ok as JsonObject).len;
      }
    }
    expect(level(probe as unknown as JsonValue)).toEqual(level(want as unknown as JsonValue));
  }, 60_000);

  const returns = Object.entries(RETURNS).filter(([, byEngine]) => byEngine[engine] !== undefined);
  it.each(returns)("returns.json %s", async (key, byEngine) => {
    const live = byEngine[engine]!;
    const loggerName = engine === "next-gen"
      ? "scripts.OAUTH2_MAY_ACT_NEXT_GEN.44cfa247-295a-4c01-8af0-ca0ff008361d.(zzemuC6-ma-ng)"
      : "scripts.OAUTH2_MAY_ACT.52eb4f64-7d62-4683-a6ff-2a03ea2b4998.(zzemuC6-ma-legacy)";
    const outcome = await run(key, live.script, loggerName, live.http === 200).then(
      (r) => r,
      (e: unknown) => e,
    );
    if (outcome instanceof CaseRunError) {
      expect(live.http, live.script).toBe(500);
      return;
    }
    const ma = (outcome as { effects: { mayAct?: MayActEffect } }).effects.mayAct!;
    expect(ma.status).toBe(live.http);
    if (live.http !== 200) {
      expect(ma.refusal).toBe(live.error_description);
      expect(ma.scope).toBeNull();
      return;
    }
    expect(ma.refusal).toBeNull();
    expect(level(claim(ma.mayActPresent, ma.mayAct))).toEqual(level(live.may_act ?? null));
    expect(level(claim(ma.actPresent, ma.act))).toEqual(level(live.act ?? null));
    expect(ma.scope).toEqual(live.scope === undefined ? null : live.scope.split(" "));
  }, 60_000);

  const faults: [string, string][] = engine === "legacy"
    ? [["token.getClientId();", "oauth2-may-act-unmeasured/token.getClientId()"]]
    : [["scopes.add(\"x\");", "oauth2-may-act-unmeasured/scopes"]];
  it.each(faults)("%s is a named fault", async (source, id) => {
    expectHarnessFault(id);
    const loggerName = engine === "next-gen"
      ? "scripts.OAUTH2_MAY_ACT_NEXT_GEN.44cfa247-295a-4c01-8af0-ca0ff008361d.(zzemuC6-ma-ng)"
      : "scripts.OAUTH2_MAY_ACT.52eb4f64-7d62-4683-a6ff-2a03ea2b4998.(zzemuC6-ma-legacy)";
    const error = await run(id, source, loggerName).then(() => null, (e: unknown) => e);
    expect(error).toBeInstanceOf(CaseRunError);
    const ids = ((error as CaseRunError).response.emulation?.faults ?? []).map((f) => f.id);
    expect(ids).toContain(id);
  }, 60_000);
});
