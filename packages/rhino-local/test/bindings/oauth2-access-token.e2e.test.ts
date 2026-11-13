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
import { expectEmulationFailure, expectHarnessFault } from "../../src/harness/emulation-guard.ts";
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

  // A list is a real Java list at every size, so an empty one answers as Java does.
  it("a size-zero request value has length 0", async () => {
    const { effects, verdict } = await runCase(runner, defineCase({
      name: `empty request ${engine}`,
      context: "OAUTH2_ACCESS_TOKEN_MODIFICATION",
      script: `var x = requestProperties.requestParams.x; accessToken.setField("probe", x.length + "|" + x.size() + "|" + String(x) + "|" + JSON.stringify(x));`,
      given: { engine, request: { params: { x: [] } } },
      expect: { accessToken: { fields: { added: { probe: "0|0|[]|[]" } } } },
    }));
    expect(verdict.pass, verdict.summary).toBe(true);
    expect(effects.accessToken!.final.fields.probe).toBe("0|0|[]|[]");
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

  const emptyList = async (name: string, source: string, extra: Record<string, unknown>) => {
    const { effects, verdict } = await runCase(runner, defineCase({
      name: `${name} ${engine}`,
      context: "OAUTH2_ACCESS_TOKEN_MODIFICATION",
      script: source,
      given: { engine, ...extra },
      expect: { accessToken: { fields: { added: { probe: "[]|0" } } } },
    }));
    expect(verdict.pass, verdict.summary).toBe(true);
    expect(effects.accessToken!.final.fields.probe).toBe("[]|0");
  };

  it("an empty audience list renders [] and has length 0", async () => {
    await emptyList(
      "empty audience",
      `accessToken.setField("probe", String(accessToken.getAudience()) + "|" + accessToken.getAudience().length);`,
      { accessToken: { audience: [] } },
    );
  }, 60_000);

  it("an empty getScope() list renders [] and has length 0", async () => {
    await emptyList(
      "empty getScope",
      `accessToken.setField("probe", String(accessToken.getScope()) + "|" + accessToken.getScope().length);`,
      { accessToken: { scope: [] } },
    );
  }, 60_000);

  // What AIC does with this write was not measured. Here it ends the run, and the error says why.
  it("an index write to the getScope() list ends the run with an error naming the refusal", async () => {
    expectEmulationFailure("java.lang.UnsupportedOperationException");
    const error = await runCase(runner, defineCase({
      name: `getScope index write ${engine}`,
      context: "OAUTH2_ACCESS_TOKEN_MODIFICATION",
      script: `try { accessToken.getScope()[0] = "z"; } catch (e) {}`,
      given: { engine, accessToken: { scope: ["a"] } },
      expect: {},
    })).then(() => null, (e: unknown) => e);
    expect(error).toBeInstanceOf(CaseRunError);
    expect((error as CaseRunError).message).toContain("java.lang.UnsupportedOperationException");
  }, 60_000);

  it("an empty scopes list renders [] and has length 0", async () => {
    await emptyList(
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

  // setField("aud") measured 2026-11-14. The issued claim is the fields diff.
  // An omitted write keeps the client-id string; null removes it.
  const CLIENT = "widget-client";
  const audCase = (name: string, script: string, fields: Record<string, unknown>) =>
    runCase(runner, defineCase({
      name,
      context: "OAUTH2_ACCESS_TOKEN_MODIFICATION",
      script,
      given: { engine, accessToken: { clientId: CLIENT } },
      expect: { accessToken: { fields } },
    }));

  it("setField(\"aud\", string) issues that string and getAudience lists it", async () => {
    const { effects, verdict } = await audCase(
      "aud string",
      [
        "var before = [];",
        "var seen = accessToken.getCustomFields();",
        "for (var k in seen) before.push(k);",
        "var ret = accessToken.setField(\"aud\", \"widget-audience\");",
        "var after = [];",
        "seen = accessToken.getCustomFields();",
        "for (var k2 in seen) after.push(k2);",
        "var field = accessToken.getField(\"aud\");",
        "var audience = accessToken.getAudience();",
        "before.sort(); after.sort();",
        "accessToken.setField(\"probe\", [typeof ret, typeof field, field, audience.length, audience[0], before.join(\",\"), after.join(\",\")].join(\"|\"));",
      ].join("\n"),
      {
        changed: { aud: "widget-audience" },
        added: { probe: "undefined|string|widget-audience|1|widget-audience|expires_in,subname|expires_in,subname" },
      },
    );
    expect(verdict.pass, verdict.summary).toBe(true);
    expect(effects.accessToken!.initial.fields.aud).toBe(CLIENT);
    expect(effects.accessToken!.final.fields.aud).toBe("widget-audience");
  }, 60_000);

  it("setField(\"aud\", one-element array) issues an array, not a string", async () => {
    const { effects, verdict } = await audCase(
      "aud one array",
      [
        "accessToken.setField(\"aud\", [\"only\"]);",
        "var field = accessToken.getField(\"aud\");",
        "var audience = accessToken.getAudience();",
        "accessToken.setField(\"probe\", [typeof field, Array.isArray(field), field.length, field[0], audience.length, audience[0]].join(\"|\"));",
      ].join("\n"),
      {
        changed: { aud: ["only"] },
        added: { probe: "object|true|1|only|1|only" },
      },
    );
    expect(verdict.pass, verdict.summary).toBe(true);
    expect(effects.accessToken!.final.fields.aud).toEqual(["only"]);
  }, 60_000);

  it("setField(\"aud\", multi-element array) keeps the order on the claim and getAudience", async () => {
    const { effects, verdict } = await audCase(
      "aud two array",
      [
        "accessToken.setField(\"aud\", [\"one\", \"two\"]);",
        "var field = accessToken.getField(\"aud\");",
        "var audience = accessToken.getAudience();",
        "accessToken.setField(\"probe\", [field.length, field[0], field[1], audience.length, audience[0], audience[1]].join(\"|\"));",
      ].join("\n"),
      {
        changed: { aud: ["one", "two"] },
        added: { probe: "2|one|two|2|one|two" },
      },
    );
    expect(verdict.pass, verdict.summary).toBe(true);
    expect(effects.accessToken!.final.fields.aud).toEqual(["one", "two"]);
  }, 60_000);

  it("setField(\"aud\", null) removes the claim and getAudience is empty", async () => {
    const { effects, verdict } = await audCase(
      "aud null",
      [
        "var ret = accessToken.setField(\"aud\", null);",
        "var audience = accessToken.getAudience();",
        "accessToken.setField(\"probe\", [typeof ret, String(accessToken.getField(\"aud\")), String(audience), audience.length].join(\"|\"));",
      ].join("\n"),
      { removed: ["aud"], added: { probe: "undefined|null|[]|0" } },
    );
    expect(verdict.pass, verdict.summary).toBe(true);
    expect(effects.accessToken!.initial.fields.aud).toBe(CLIENT);
    expect(effects.accessToken!.final.fields.aud).toBeUndefined();
    expect(effects.accessToken!.final.fields).not.toHaveProperty("aud");
  }, 60_000);

  it("an omitted aud write leaves the client-id claim in place", async () => {
    const { effects, verdict } = await audCase(
      "aud omitted",
      [
        "var audience = accessToken.getAudience();",
        "accessToken.setField(\"probe\", [accessToken.getField(\"aud\"), audience.length, audience[0]].join(\"|\"));",
      ].join("\n"),
      { added: { probe: `${CLIENT}|1|${CLIENT}` } },
    );
    expect(verdict.pass, verdict.summary).toBe(true);
    expect(effects.accessToken!.initial.fields.aud).toBe(CLIENT);
    expect(effects.accessToken!.final.fields.aud).toBe(CLIENT);
  }, 60_000);

  it("removeField is absent and aud stays the client id", async () => {
    const { effects, verdict } = await audCase(
      "aud removeField",
      [
        "var message = \"\";",
        "try { accessToken.removeField(\"aud\"); } catch (e) {",
        "  message = String(e).split(\" in object \")[0].replace(/ \\([^()]*#\\d+\\)$/, \"\");",
        "}",
        "accessToken.setField(\"probe\", (typeof accessToken.removeField) + \"|\" + message);",
      ].join("\n"),
      { added: { probe: "undefined|TypeError: Cannot find function removeField" } },
    );
    expect(verdict.pass, verdict.summary).toBe(true);
    expect(effects.accessToken!.initial.fields.aud).toBe(CLIENT);
    expect(effects.accessToken!.final.fields.aud).toBe(CLIENT);
  }, 60_000);

  // identity.setAttribute / store measured 2026-11-14. A read between the two
  // still sees the seeded value. store records expect.identityWrites.
  const owner = {
    engine,
    accessToken: { clientId: CLIENT },
    identity: { name: "user-1", attributes: { description: ["baseline"] } },
  };

  it("setAttribute is invisible until store, which records the write", async () => {
    const { effects, verdict } = await runCase(runner, defineCase({
      name: "identity store",
      context: "OAUTH2_ACCESS_TOKEN_MODIFICATION",
      script: [
        "var before = identity.getAttributeValues(\"description\");",
        "var setRet = identity.setAttribute(\"description\", [\"stored\"]);",
        "var mid = identity.getAttributeValues(\"description\");",
        "var storeRet = identity.store();",
        "var after = identity.getAttributeValues(\"description\");",
        "accessToken.setField(\"probe\", [typeof setRet, typeof storeRet, before[0], mid[0], after[0]].join(\"|\"));",
      ].join("\n"),
      given: owner,
      expect: {
        accessToken: { fields: { added: { probe: "undefined|undefined|baseline|baseline|stored" } } },
        identityWrites: [{ identity: "user-1", attribute: "description", values: ["stored"] }],
      },
    }));
    expect(verdict.pass, verdict.summary).toBe(true);
    expect(effects.identityWrites).toEqual([
      { identity: "user-1", attribute: "description", values: ["stored"] },
    ]);
  }, 60_000);

  it("setAttribute without store leaves the attribute and records no write", async () => {
    const { effects, verdict } = await runCase(runner, defineCase({
      name: "identity no store",
      context: "OAUTH2_ACCESS_TOKEN_MODIFICATION",
      script: [
        "var before = identity.getAttributeValues(\"description\")[0];",
        "identity.setAttribute(\"description\", [\"stored\"]);",
        "var mid = identity.getAttributeValues(\"description\")[0];",
        "accessToken.setField(\"probe\", before + \"|\" + mid);",
      ].join("\n"),
      given: owner,
      expect: { accessToken: { fields: { added: { probe: "baseline|baseline" } } } },
    }));
    expect(verdict.pass, verdict.summary).toBe(true);
    expect(effects.identityWrites).toEqual([]);
  }, 60_000);

  it("identity.addAttribute stays unmeasured", async () => {
    expectHarnessFault("oauth2-token-unmeasured/identity.addAttribute()");
    await expect(runCase(runner, defineCase({
      name: "identity addAttribute",
      context: "OAUTH2_ACCESS_TOKEN_MODIFICATION",
      script: "identity.addAttribute(\"description\", \"extra\");",
      given: owner,
      expect: {},
    }))).rejects.toThrow(/identity\.addAttribute/);
  }, 60_000);
});
