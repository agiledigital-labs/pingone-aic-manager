/**
 * The access-token modification context on the real JVM, compared with live
 * captures. Each probe in test/fixtures/oauth2-atm/ ran unchanged on a sandbox
 * tenant (2026-11-10, a stateless client_credentials token) and
 * its `<probe>.<engine>.json` holds what AIC answered: the probe's own record
 * (the `aicprobe` claim), the JWT's claims minus the per-token ones, and the
 * token endpoint's response body. Here the same script runs in the emulator
 * with a fixture describing that client, and every observable must agree.
 */
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { CaseRunError, runCase } from "../../src/bindings/run.ts";
import { defineCase } from "../../src/case/validate.ts";
import { expectHarnessFault } from "../../src/harness/emulation-guard.ts";
import type { AccessTokenEffect, Engine, JsonObject, JsonValue } from "../../src/case/types.ts";
import { packageRoot } from "../../src/paths.ts";
import { RhinoRunner } from "../../src/runner.ts";

const dir = join(packageRoot, "test", "fixtures", "oauth2-atm");
const script = (probe: string) => readFileSync(join(dir, `${probe}.script.js`), "utf8");

interface Capture {
  http: number;
  body: JsonObject;
  claims?: JsonObject;
  expMinusIat?: number;
  probe?: JsonObject;
}
const capture = (probe: string, engine: Engine) =>
  JSON.parse(readFileSync(join(dir, `${probe}.${engine}.json`), "utf8")) as Capture;

// The sandbox client the probes ran against, as a fixture. The response-type
// list is only counted by the probe, so its members are placeholders.
const given = (engine: Engine) => ({
  engine,
  accessToken: { clientId: "zzemuC-atm", scope: ["zzemuc-probe"] },
  identity: { attributes: { "com.forgerock.openam.oauth2provider.clientType": ["Confidential"] } },
  client: {
    allowedScopes: ["zzemuc-other", "zzemuc-probe"],
    allowedResponseTypes: ["r1", "r2", "r3", "r4", "r5", "r6", "r7", "r8", "r9"],
  },
  request: { headers: { "Content-Type": ["application/x-www-form-urlencoded"] } },
});

// Claims AM writes on every token; the rest of the JWT is what the script set.
const AM_CLAIMS = new Set([
  "aud", "client_id", "cts", "expires_in", "grant_type", "realm", "scope", "sub", "subname", "tokenName", "token_type",
]);
const STATE_CLAIMS: Record<string, keyof AccessTokenEffect["final"]> = {
  nonce: "nonce",
  auth_level: "authLevel",
  claims: "claims",
  act: "act",
  may_act: "mayAct",
  permissions: "permissions",
};

/** The JWT the emulated token would become, in the capture's terms. */
function asIssued(token: AccessTokenEffect): { claims: JsonObject; body: JsonObject; scope: string[]; lifetime: number } {
  const claims: JsonObject = {};
  for (const [key, value] of Object.entries(token.final.fields)) {
    if (key !== "aicprobe" && !AM_CLAIMS.has(key)) claims[key] = value;
  }
  for (const [claim, key] of Object.entries(STATE_CLAIMS)) {
    const value = token.final[key] as JsonValue;
    if (value !== null) claims[claim] = value;
  }
  return { claims, body: token.final.responseFields, scope: token.final.scope, lifetime: 0 };
}

describe("access-token modification matches the live capture", () => {
  const engine = "next-gen" as const;
  let runner: RhinoRunner;
  beforeAll(async () => { runner = await RhinoRunner.spawn(); }, 60_000);
  afterAll(async () => { await runner?.close(); }, 30_000);

  const run = (probe: string) =>
    runCase(runner, defineCase({
      name: `${probe} ${engine}`,
      context: "OAUTH2_ACCESS_TOKEN_MODIFICATION",
      script: script(probe),
      given: given(engine),
      expect: { allowUndeclared: { accessToken: true, logs: true } },
    }));

  it.each(["surface", "setters", "scope"])("%s: the probe's record, the claims, the scope and the body", async (probe) => {
    const live = capture(probe, engine);
    expect(live.http).toBe(200);
    const { effects, verdict } = await run(probe);
    expect(verdict.pass, verdict.summary).toBe(true);
    const token = effects.accessToken!;
    expect(token.refusal).toBeNull();
    expect(JSON.parse(token.final.fields.aicprobe as string)).toEqual(live.probe!);
    const issued = asIssued(token);
    const liveClaims = Object.fromEntries(Object.entries(live.claims!).filter(([key]) => !AM_CLAIMS.has(key)));
    expect(issued.claims).toEqual(liveClaims);
    expect([...issued.scope].sort()).toEqual([...(live.claims!.scope as string[])].sort());
    const { expires_in: _expires, scope: _scope, token_type: _type, ...extras } = live.body;
    expect(issued.body).toEqual(extras);
    // The JWT's lifetime is the expiry the script left, from the auth time.
    const authMs = token.final.expiryTime - token.initial.expiryTime + 3_600_000;
    expect(authMs / 1000).toBe(live.expMinusIat);
  }, 60_000);

  it("identity-string: String(identity) converts, identity.toString() does not exist", async () => {
    const live = capture("identity-string", engine);
    expect(live.http).toBe(200);
    const { effects } = await run("identity-string");
    const local = JSON.parse(effects.accessToken!.final.fields.aicprobe as string) as JsonObject;
    // The identity hash code is per object; only its presence is the claim.
    const hashless = JSON.parse(JSON.stringify(live.probe).replace(/@[0-9a-f]+/g, "@rhino-local")) as JsonObject;
    expect(local).toEqual(hashless);
  }, 60_000);


  it("remove-scopes: the script returns, and AM refuses to issue the token", async () => {
    const live = capture("remove-scopes", engine);
    expect(live.http).toBe(500);
    const { effects } = await run("remove-scopes");
    expect(effects.accessToken!.refusal).toBe(live.body.error_description);
  }, 60_000);

  it("throws: an uncaught throw fails the run, as AM fails the token request", async () => {
    const live = capture("throws", engine);
    expect(live.http).toBe(500);
    expect(live.body.error_description).toBe("Error running access token modification plugin");
    await expect(run("throws")).rejects.toThrow(CaseRunError);
    await expect(run("throws")).rejects.toThrow(/zz deliberate/);
  }, 60_000);

  // Sol ctx r10 P2-1. No capture measured a size-zero request value. An empty
  // identity attribute is a different list and does not permit this one.
  it("r10 P2-1 a size-zero request value's length is a named fault", async () => {
    const id = "oauth2-token-unmeasured/requestProperties";
    expectHarnessFault(id);
    const error = await runCase(runner, defineCase({
      name: `r10 empty request ${engine}`,
      context: "OAUTH2_ACCESS_TOKEN_MODIFICATION",
      script: `accessToken.setField("probe", requestProperties.requestParams.x.length);`,
      given: { engine, request: { params: { x: [] } } },
      expect: { accessToken: { fields: { added: { probe: 0 } } } },
    })).then(() => null, (e: unknown) => e);
    expect(error).toBeInstanceOf(CaseRunError);
    const faults = (error as CaseRunError).response.emulation?.faults ?? [];
    expect(faults.map((f) => f.id)).toContain(id);
    const text = faults.map((f) => f.message).join("\n");
    expect(text).toContain("this route on a size-zero list was not measured");
    expect(text).not.toContain("never produced on AIC");
  }, 60_000);

  it("r10 P2-1 a non-empty request value still answers length", async () => {
    const { effects, verdict } = await runCase(runner, defineCase({
      name: `r10 request length ${engine}`,
      context: "OAUTH2_ACCESS_TOKEN_MODIFICATION",
      script: `accessToken.setField("probe", requestProperties.requestParams.grant_type.length);`,
      given: { engine },
      expect: { accessToken: { fields: { added: { probe: 1 } } } },
    }));
    expect(verdict.pass, verdict.summary).toBe(true);
    expect(effects.accessToken!.final.fields.probe).toBe(1);
  }, 60_000);

  it("r10 P2-1 an empty identity attribute still answers length and String", async () => {
    const { effects, verdict } = await runCase(runner, defineCase({
      name: `r10 empty attribute ${engine}`,
      context: "OAUTH2_ACCESS_TOKEN_MODIFICATION",
      script: `var v = identity.getAttributeValues("zzNoSuchAttr"); accessToken.setField("probe", v.length + "|" + String(v));`,
      given: { engine },
      expect: { accessToken: { fields: { added: { probe: "0|[]" } } } },
    }));
    expect(verdict.pass, verdict.summary).toBe(true);
    expect(effects.accessToken!.final.fields.probe).toBe("0|[]");
  }, 60_000);

  // Sol ctx r10 P2-2. Rhino names a regexp argument "function" (it implements
  // java Function) even though script typeof is "object".
  it("r10 P2-2 a regexp argument is function in the overload signature", async () => {
    const { effects, verdict } = await runCase(runner, defineCase({
      name: `r10 regexp ${engine}`,
      context: "OAUTH2_ACCESS_TOKEN_MODIFICATION",
      script: `try { accessToken.getField(/x/, 1); } catch (e) { accessToken.setField("diagnostic", String(e).indexOf("getField(function,number)") >= 0); }`,
      given: { engine },
      expect: { accessToken: { fields: { added: { diagnostic: true } } } },
    }));
    expect(verdict.pass, verdict.summary).toBe(true);
    expect(effects.accessToken!.final.fields.diagnostic).toBe(true);
  }, 60_000);

  // Sol ctx r11 P2-1. The empty identity-attribute surface does not measure
  // getAudience, getScope or scopes. Each of these is a reproduction.
  const emptyListFault = async (name: string, source: string, extra: Record<string, unknown>) => {
    const id = "oauth2-token-unmeasured/list";
    expectHarnessFault(id);
    const error = await runCase(runner, defineCase({
      name: `r11 ${name} ${engine}`,
      context: "OAUTH2_ACCESS_TOKEN_MODIFICATION",
      script: source,
      given: { engine, ...extra },
      expect: { accessToken: { fields: { added: { probe: "[]|0" } } } },
    })).then(() => null, (e: unknown) => e);
    expect(error).toBeInstanceOf(CaseRunError);
    const faults = (error as CaseRunError).response.emulation?.faults ?? [];
    expect(faults.map((f) => f.id)).toContain(id);
    const text = faults.map((f) => f.message).join("\n");
    expect(text).toContain("this route on a size-zero list was not measured");
    expect(text).not.toContain("never produced on AIC");
  };

  it("r11 P2-1 an empty audience list's surface is a named fault", async () => {
    await emptyListFault(
      "empty audience",
      `accessToken.setField("probe", String(accessToken.getAudience()) + "|" + accessToken.getAudience().length);`,
      { accessToken: { audience: [] } },
    );
  }, 60_000);

  it("r11 P2-1 an empty getScope() list's surface is a named fault", async () => {
    await emptyListFault(
      "empty getScope",
      `accessToken.setField("probe", String(accessToken.getScope()) + "|" + accessToken.getScope().length);`,
      { accessToken: { scope: [] } },
    );
  }, 60_000);

  it("r11 P2-1 an empty scopes list's surface is a named fault", async () => {
    await emptyListFault(
      "empty scopes",
      `accessToken.setField("probe", String(scopes) + "|" + scopes.length);`,
      { scopes: [] },
    );
  }, 60_000);

  // Sol ctx r11 P2-2. A Java constructor is a Wrapper; Rhino names
  // java.lang.Class. A method reference stays function.
  const overloadName = async (name: string, value: string, needle: string) => {
    const { effects, verdict } = await runCase(runner, defineCase({
      name: `r11 ${name} ${engine}`,
      context: "OAUTH2_ACCESS_TOKEN_MODIFICATION",
      script: `try { accessToken.getField(${value}, 1); } catch (e) { accessToken.setField("diagnostic", String(e).indexOf(${JSON.stringify(needle)}) >= 0); }`,
      given: { engine },
      expect: { accessToken: { fields: { added: { diagnostic: true } } } },
    }));
    expect(verdict.pass, verdict.summary).toBe(true);
    expect(effects.accessToken!.final.fields.diagnostic).toBe(true);
  };

  it("r11 P2-2 java.lang.String constructor is java.lang.Class in the overload signature", async () => {
    await overloadName("string ctor", "java.lang.String", "getField(java.lang.Class,number)");
  }, 60_000);

  it("r11 P2-2 a Java method reference stays function in the overload signature", async () => {
    await overloadName("method ref", "(new java.lang.String(\"s\")).isEmpty", "getField(function,number)");
  }, 60_000);
});
