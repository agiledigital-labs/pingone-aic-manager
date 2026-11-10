/**
 * The access-token modification context on the real JVM, compared with live
 * captures. Each probe in test/fixtures/oauth2-atm/ ran unchanged on a sandbox
 * tenant (2026-11-10, both engines, a stateless client_credentials token) and
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

describe.each(["next-gen", "legacy"] as const)("access-token modification on %s matches the live capture", (engine) => {
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

  it.runIf(engine === "next-gen")("identity-string: String(identity) converts, identity.toString() does not exist", async () => {
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
});

describe("legacy identity.getAttribute (docs/api/12, measured 2026-09-10)", () => {
  let runner: RhinoRunner;
  beforeAll(async () => { runner = await RhinoRunner.spawn(); }, 60_000);
  afterAll(async () => { await runner?.close(); }, 30_000);

  it("returns a HashSet read through toArray(), and an empty set for an attribute the identity lacks", async () => {
    const { effects } = await runCase(runner, defineCase({
      name: "getAttribute shapes",
      context: "OAUTH2_ACCESS_TOKEN_MODIFICATION",
      script: `var held = identity.getAttribute("thing");
var missing = identity.getAttribute("zzNoSuchAttr");
var index; try { held[0]; index = "no throw"; } catch (e) { index = String(e); }
accessToken.setField("out", [String(held.toArray()[0]), held.size(), String(held), missing.size(), missing.isEmpty(),
  String(missing), typeof held.length, index].join("|"));`,
      given: { engine: "legacy", identity: { attributes: { thing: ["Confidential"] } } },
      expect: { allowUndeclared: { accessToken: true } },
    }));
    expect(effects.accessToken!.final.fields.out).toBe(
      'Confidential|1|[Confidential]|0|true|[]|undefined|InternalError: Java class "java.util.HashSet" has no public instance field or method named "0".'
    );
  }, 60_000);
});
