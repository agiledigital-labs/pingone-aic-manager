/**
 * The OIDC claims context on the real JVM, compared with live captures. Each
 * probe in test/fixtures/oidc-claims/ ran unchanged on a sandbox tenant
 * (2026-11-10): a password-grant token request, whose id token
 * carries the script's claims, then a userinfo call with the access token,
 * which runs the script again. `<probe>.<engine>.json` holds what AIC
 * answered: the id token's claims, the userinfo body, and the probe's own
 * record from each run (the `aicprobe` claim). `returns.json` holds one-line
 * scripts that pin what AM does with each kind of result. Here the same
 * scripts run in the emulator with a fixture describing that client, user and
 * request, and every observable must agree.
 */
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { CaseRunError, runCase } from "../../src/bindings/run.ts";
import { expectHarnessFault } from "../../src/harness/emulation-guard.ts";
import { defineCase } from "../../src/case/validate.ts";
import type { Engine, JsonObject } from "../../src/case/types.ts";
import { packageRoot } from "../../src/paths.ts";
import { RhinoRunner } from "../../src/runner.ts";

const dir = join(packageRoot, "test", "fixtures", "oidc-claims");
const script = (probe: string) => readFileSync(join(dir, `${probe}.script.js`), "utf8");

interface Capture {
  http: number;
  idToken: JsonObject | null;
  userinfo: JsonObject | null;
  idTokenProbe: JsonObject | null;
  userinfoProbe: JsonObject | null;
}
const capture = (probe: string, engine: Engine) =>
  JSON.parse(readFileSync(join(dir, `${probe}.${engine}.json`), "utf8")) as Capture;

interface Returned {
  script: string;
  http: number;
  body: JsonObject;
  idToken: JsonObject | null;
  userinfo: JsonObject | null;
}
const RETURNS = JSON.parse(readFileSync(join(dir, "returns.json"), "utf8")) as Record<
  string,
  Partial<Record<Engine, Returned>>
>;

// The sandbox user's id, which AM used as its name and the id token subject.
const SUB = capture("surface", "next-gen").idToken!.sub as string;
const SCOPES = ["openid", "profile", "zzemuc2-probe", "email"];
// Nineteen request headers reached the script at the token endpoint and
// eighteen at userinfo (no body, so no Content-Type). The probes count them
// and read only Content-Type, so the rest are placeholders.
const headers = (count: number, contentType: boolean) => {
  const out: Record<string, string[]> = contentType ? { "Content-Type": ["application/x-www-form-urlencoded"] } : {};
  for (let i = Object.keys(out).length; i < count; i += 1) out[`X-Placeholder-${i}`] = ["v"];
  return out;
};

type Endpoint = "token" | "userinfo";
const given = (engine: Engine, endpoint: Endpoint, claimsLocales: string[] = []) => ({
  engine,
  scriptName: "zzemuC2-oidc-ng",
  identity: { name: SUB, attributes: { mail: ["zzemuc2@example.com"], givenName: ["Zed"] } },
  scopes: SCOPES,
  client: {
    clientId: "zzemuC2-oidc",
    allowedScopes: ["email", "openid", "profile", "zzemuc2-other", "zzemuc2-probe"],
    allowedResponseTypes: ["r1", "r2", "r3", "r4", "r5", "r6", "r7", "r8", "r9"],
  },
  request: endpoint === "token"
    ? {
      params: {
        client_id: ["zzemuC2-oidc"],
        grant_type: ["password"],
        scope: ["openid profile email zzemuc2-probe"],
        username: ["zzemuC2user"],
      },
      headers: headers(19, true),
    }
    : {
      params: {},
      headers: headers(18, false),
      uri: "https://tenant.example.com/am/oauth2/realms/root/realms/alpha/userinfo",
    },
  oidc: { claimsLocales: endpoint === "token" ? claimsLocales : [] },
});

// Claims AM writes into every id token; the rest is what the script returned.
const AM_CLAIMS = new Set(["aud", "azp", "realm", "tokenName", "tokenType"]);
const scriptClaims = (token: JsonObject) =>
  Object.fromEntries(Object.entries(token).filter(([key]) => !AM_CLAIMS.has(key) && key !== "aicprobe"));
const withoutProbe = (body: JsonObject) => Object.fromEntries(Object.entries(body).filter(([key]) => key !== "aicprobe"));

describe("OIDC claims matches the live capture", () => {
  const engine = "next-gen" as const;
  let runner: RhinoRunner;
  beforeAll(async () => { runner = await RhinoRunner.spawn(); }, 60_000);
  afterAll(async () => { await runner?.close(); }, 30_000);

  const run = (name: string, source: string, endpoint: Endpoint, claimsLocales?: string[]) =>
    runCase(runner, defineCase({
      name: `${name} ${engine} ${endpoint}`,
      context: "OIDC_CLAIMS",
      script: source,
      given: given(engine, endpoint, claimsLocales),
      expect: { allowUndeclared: { oidc: true, logs: true } },
    }));

  // `delete claims.subname` answers false on AIC and keeps the key; a JS
  // object cannot do that while remove() works, so the emulator faults it
  // (tested below) and this comparison runs the probe without that one line.
  const probeSource = (probe: string) =>
    probe === "claimsmap"
      ? script(probe).replace(/^out\.deleteSub = .*\n/m, "")
      : script(probe);
  const dropped = (probe: string) => (probe === "claimsmap" ? ["deleteSub"] : []);

  it.each(["surface", "requested", "claimsmap"])("%s: the probe's record at both endpoints, and the claims", async (probe) => {
    const live = capture(probe, engine);
    expect(live.http).toBe(200);
    for (const endpoint of ["token", "userinfo"] as const) {
      const { effects, verdict } = await run(probe, probeSource(probe), endpoint,
        probe === "requested" ? ["fr-CA", "en"] : []);
      expect(verdict.pass, verdict.summary).toBe(true);
      const oidc = effects.oidc!;
      expect(oidc.refusal).toBeNull();
      const record = JSON.parse(oidc.userinfo.aicprobe as string) as JsonObject;
      const want = { ...(endpoint === "token" ? live.idTokenProbe! : live.userinfoProbe!) };
      for (const key of dropped(probe)) delete want[key];
      expect(record, endpoint).toEqual(want);
      if (endpoint === "token") {
        expect(scriptClaims(oidc.idToken)).toEqual(scriptClaims(live.idToken!));
      } else {
        expect(withoutProbe(oidc.userinfo)).toEqual(withoutProbe(live.userinfo!));
      }
    }
  }, 60_000);

  it.each(["values", "sideeffect"])("%s: the userinfo body and the id token's script claims", async (probe) => {
    const live = capture(probe, engine);
    const { effects } = await run(probe, script(probe), "token");
    expect(effects.oidc!.userinfo).toEqual(live.userinfo);
    expect(scriptClaims(effects.oidc!.idToken)).toEqual(scriptClaims(live.idToken!));
  }, 60_000);

  const returns = Object.entries(RETURNS).filter(([, byEngine]) => byEngine[engine] !== undefined);
  it.each(returns)("returns.json %s", async (name, byEngine) => {
    const live = byEngine[engine]!;
    if (live.http === 200) {
      const { effects } = await run(name, live.script, "token");
      expect(effects.oidc!.refusal).toBeNull();
      expect(effects.oidc!.userinfo).toEqual(live.userinfo);
      expect(scriptClaims(effects.oidc!.idToken)).toEqual(scriptClaims(live.idToken!));
      return;
    }
    expect(live.http).toBe(500);
    const description = live.body.error_description as string;
    if (description === "Error running OIDC claims plugin" && /throw /.test(live.script)) {
      // An uncaught throw fails the run, as AM fails the token request.
      await expect(run(name, live.script, "token")).rejects.toThrow(CaseRunError);
      return;
    }
    const { effects } = await run(name, live.script, "token");
    expect(effects.oidc!.refusal).toBe(description);
    expect(effects.oidc!.returned).toBe("none");
  }, 60_000);

  it("delete on the claims map is a named fault, not a silent removal", async () => {
    expectHarnessFault("oidc-claims-unmeasured/claims.subname");
    const error = await run("delete", `delete claims.subname; ({ values: { a: "b" } });`, "token")
      .then(() => null, (e: unknown) => e);
    expect(error).toBeInstanceOf(CaseRunError);
    const ids = ((error as CaseRunError).response.emulation?.faults ?? []).map((f) => f.id);
    expect(ids).toContain("oidc-claims-unmeasured/claims.subname");
  }, 60_000);

  const claimsReturn = (probe: string) => `({ values: { probe: ${probe} } });`;

  // A list is a real Java list at every size, so an empty one answers as Java does.
  const answers = async (name: string, probe: string, userinfo: { probe: string | boolean }, given: Record<string, unknown> = { scopes: [] }) => {
    const { effects, verdict } = await runCase(runner, defineCase({
      name: `${name} ${engine}`,
      context: "OIDC_CLAIMS",
      script: claimsReturn(probe),
      given: { engine, ...given },
      expect: { oidc: { userinfo } },
    }));
    expect(verdict.pass, verdict.summary).toBe(true);
    expect(effects.oidc!.userinfo).toEqual(userinfo);
  };

  it("a size-zero request value has length 0", async () => {
    await answers("empty request", "String(requestProperties.requestParams.x.length)", { probe: "0" }, { request: { params: { x: [] } } });
  }, 60_000);

  it("r10 P2-1 an empty claimsLocales list still answers length and String", async () => {
    const { effects, verdict } = await runCase(runner, defineCase({
      name: `r10 claims locales ${engine}`,
      context: "OIDC_CLAIMS",
      script: claimsReturn("claimsLocales.length + \"|\" + String(claimsLocales)"),
      given: { engine },
      expect: { oidc: { userinfo: { probe: "0|[]" } } },
    }));
    expect(verdict.pass, verdict.summary).toBe(true);
    expect(effects.oidc!.userinfo).toEqual({ probe: "0|[]" });
  }, 60_000);

  it("an empty scopes list renders [] and has length 0", async () => {
    await answers("empty scopes", "String(scopes) + \"|\" + scopes.length", { probe: "[]|0" });
  }, 60_000);

  it("r11 an empty missing-attribute list still answers length and String", async () => {
    const { effects, verdict } = await runCase(runner, defineCase({
      name: `r11 missing attribute ${engine}`,
      context: "OIDC_CLAIMS",
      script: claimsReturn("(function () { var v = identity.getAttributeValues(\"zzNoSuchAttr\"); return v.length + \"|\" + String(v); })()"),
      given: { engine },
      expect: { oidc: { userinfo: { probe: "0|[]" } } },
    }));
    expect(verdict.pass, verdict.summary).toBe(true);
    expect(effects.oidc!.userinfo).toEqual({ probe: "0|[]" });
  }, 60_000);

  it("r11 an empty redirect-URI list still answers length and String", async () => {
    const { effects, verdict } = await runCase(runner, defineCase({
      name: `r11 redirect ${engine}`,
      context: "OIDC_CLAIMS",
      script: claimsReturn("clientProperties.redirectUris.length + \"|\" + String(clientProperties.redirectUris)"),
      given: { engine },
      expect: { oidc: { userinfo: { probe: "0|[]" } } },
    }));
    expect(verdict.pass, verdict.summary).toBe(true);
    expect(effects.oidc!.userinfo).toEqual({ probe: "0|[]" });
  }, 60_000);


  it("an empty scopes list: toArray() has length 0, contains() is false", async () => {
    await answers("empty toArray length", "String(scopes.toArray().length)", { probe: "0" });
    await answers("empty contains", "scopes.contains(\"read\")", { probe: false });
  }, 60_000);
});

describe("one JVM running OIDC claims cases between other contexts' cases", () => {
  let runner: RhinoRunner;
  beforeAll(async () => { runner = await RhinoRunner.spawn(); }, 60_000);
  afterAll(async () => { await runner?.close(); }, 30_000);

  // Everything the OIDC seed binds, deletes, rebinds or installs.
  const surface = "JSON.stringify([typeof openidm, typeof logger.info, typeof logger.message, typeof logger.getName, "
    + "typeof claims, typeof session, typeof requestedClaims, typeof claimObjects, typeof sharedState, "
    + "typeof nodeState, typeof org.forgerock.oauth2, String(org.forgerock.oauth2)])";
  const nodeProbe = async () => {
    const { effects } = await runCase(runner, defineCase({
      name: "isolation decision node",
      script: `nodeState.putShared("out", ${surface}); action.goTo("true");`,
      expect: { outcome: "true", allowUndeclared: { sharedState: true } },
    }));
    return effects.sharedState.final.out;
  };
  const oidcCase = () => runCase(runner, defineCase({
    name: "isolation oidc",
    context: "OIDC_CLAIMS",
    script: `({ values: { a: "b" } });`,
    expect: { oidc: { userinfo: { a: "b" } } },
  }));

  it("leaves a decision node exactly as a fresh runner sees it", async () => {
    const before = await nodeProbe();
    await oidcCase();
    expect(await nodeProbe()).toBe(before);
  }, 60_000);
});
