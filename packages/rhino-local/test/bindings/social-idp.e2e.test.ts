/**
 * The social IdP profile transformation context on the real JVM, compared
 * with live captures. surface.script.js ran unchanged as a social identity
 * provider's `transform` script on a sandbox tenant (2026-11-10), during a headless social login: an OAuth2 client in one realm
 * was the provider, a journey in the other ran SelectIdPNode,
 * SocialProviderHandlerNodeV2 and a scripted decision node that read what
 * the handler left. `surface.<engine>.json` holds the probe's own record
 * (`transformProbe`, carried out in the returned profile's `aicprobe`), the
 * normalised profile as the handler's node script saw it, and which node
 * state keys the decision node found added. `returns.json` holds one-line
 * scripts that pin what AM does with each kind of result. Here the same
 * scripts run in the emulator with a fixture describing that provider
 * profile and request, and every observable must agree.
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

const dir = join(packageRoot, "test", "fixtures", "social-idp");
const SURFACE = readFileSync(join(dir, "surface.script.js"), "utf8");

interface Capture {
  http: number;
  normalizedProfile: string;
  transformProbe: Record<string, JsonValue>;
  sharedStateAdded: string[];
}
const capture = (engine: Engine) => JSON.parse(readFileSync(join(dir, `surface.${engine}.json`), "utf8")) as Capture;

interface Returned {
  script: string;
  http: number;
  normalizedProfile: string | null;
  sharedStateAdded: string[] | null;
}
const RETURNS = JSON.parse(readFileSync(join(dir, "returns.json"), "utf8")) as Record<
  string,
  Partial<Record<Engine, Returned>>
>;

// The provider's userinfo, in its own order, as String(rawProfile) rendered it.
const RAW = JSON.parse(
  /^object\|\[object JavaMap\]\|(.*)\|len=undefined$/.exec(capture("next-gen").transformProbe.raw as string)![1]!
) as JsonObject;
// AM's rendering of a normalised profile: JSON, with numbers as doubles.
const amProfile = (text: string) => JSON.parse(text) as JsonObject;
const withoutProbe = (o: JsonObject) => Object.fromEntries(Object.entries(o).filter(([k]) => k !== "aicprobe"));

const IDP = "zzemuC3-idp";
const placeholders = (count: number) => {
  const out: Record<string, string[]> = {};
  for (let i = 0; i < count; i += 1) out[`x-placeholder-${i}`] = ["v"];
  return out;
};
const given = (engine: Engine) => {
  const loggerName = capture(engine).transformProbe.loggerName as string;
  return {
    engine,
    realm: "/alpha",
    scriptName: "zzemuC3-social-ng",
    loggerScriptId: /\.([0-9a-f-]{36})\./.exec(loggerName)![1]!,
    // The handler node's own key; its value was not read.
    sharedState: { expectProfileInformation: true },
    // Five parameters and twenty-one headers reached the script; the probe counts them.
    requestParameters: placeholders(5),
    requestHeaders: placeholders(21),
    socialIdp: { rawProfile: RAW, selectedIdp: IDP },
  };
};

// Keys the run added to the node state (shared and transient), as the
// decision node after the handler saw them.
const added = (effects: { sharedState: { final: JsonObject }; transientState: { final: JsonObject } }) =>
  [...Object.keys(effects.sharedState.final), ...Object.keys(effects.transientState.final)]
    .filter((k) => k !== "expectProfileInformation" && k !== "selectedIdp")
    .sort();

describe("social IdP profile transformation matches the live capture", () => {
  const engine = "next-gen" as const;
  let runner: RhinoRunner;
  beforeAll(async () => { runner = await RhinoRunner.spawn(); }, 60_000);
  afterAll(async () => { await runner?.close(); }, 30_000);

  const run = (name: string, source: string) =>
    runCase(runner, defineCase({
      name: `${name} ${engine}`,
      context: "SOCIAL_IDP_PROFILE_TRANSFORMATION",
      script: source,
      given: given(engine),
      expect: { allowUndeclared: { socialIdp: true, sharedState: true, transientState: true, logs: true } },
    }));

  it("surface: the probe's record, the profile and the node state", async () => {
    const live = capture(engine);
    expect(live.http).toBe(200);
    const { effects, verdict } = await run("surface", SURFACE);
    expect(verdict.pass, verdict.summary).toBe(true);
    const social = effects.socialIdp!;
    expect(social.refusal).toBeNull();
    const record = JSON.parse(social.profile!.aicprobe as string) as Record<string, JsonValue>;
    const want = { ...live.transformProbe };
    // The script context object's error names AM's own script id and line.
    (want.typeof as JsonObject).context = (record.typeof as JsonObject).context!;
    // AIC lists the keys in the Java map's hash order; the view keeps the
    // profile's order. The same object either way.
    const json = (shape: JsonValue) => JSON.parse(/^string\|\[object String\]\|(.*)\|len=\d+$/.exec(shape as string)![1]!) as JsonObject;
    expect(json(record.rawJson!)).toEqual(json(want.rawJson!));
    delete want.rawJson;
    delete record.rawJson;
    expect(record).toEqual(want);
    expect(withoutProbe(social.profile!)).toEqual(withoutProbe(amProfile(live.normalizedProfile)));
    expect(added(effects)).toEqual([...live.sharedStateAdded].sort());
  }, 60_000);

  const returns = Object.entries(RETURNS).filter(([, byEngine]) => byEngine[engine] !== undefined);
  it.each(returns)("returns.json %s", async (name, byEngine) => {
    const live = byEngine[engine]!;
    if (live.http === 200) {
      const { effects } = await run(name, live.script);
      expect(effects.socialIdp!.refusal).toBeNull();
      expect(effects.socialIdp!.profile).toEqual(amProfile(live.normalizedProfile!));
      expect(added(effects)).toEqual([...live.sharedStateAdded!].sort());
      return;
    }
    expect(live.http).toBe(401);
    if (/throw /.test(live.script)) {
      // An uncaught throw fails the run, as AM fails the journey.
      await expect(run(name, live.script)).rejects.toThrow(CaseRunError);
      return;
    }
    const { effects } = await run(name, live.script);
    expect(effects.socialIdp!.returned).toBe("none");
    expect(effects.socialIdp!.refusal).toMatch(/AM fails the journey/);
  }, 60_000);

  it("a result kind nobody measured is a named fault", async () => {
    expectHarnessFault("social-idp-unmeasured/result");
    const error = await run("number", "42;").then(() => null, (e: unknown) => e);
    expect(error).toBeInstanceOf(CaseRunError);
    const ids = ((error as CaseRunError).response.emulation?.faults ?? []).map((f) => f.id);
    expect(ids).toContain("social-idp-unmeasured/result");
  }, 60_000);

  it("reading a profile value that is not a string is a named fault", async () => {
    const id = "social-idp-unmeasured/rawProfile.verified";
    expectHarnessFault(id);
    const source = `rawProfile.verified; ({});`;
    const error = await runCase(runner, defineCase({
      name: `non-string ${engine}`,
      context: "SOCIAL_IDP_PROFILE_TRANSFORMATION",
      script: source,
      given: { engine, socialIdp: { rawProfile: { verified: true } } },
      expect: { socialIdp: { profile: {} } },
    })).then(() => null, (e: unknown) => e);
    expect(error).toBeInstanceOf(CaseRunError);
    const ids = ((error as CaseRunError).response.emulation?.faults ?? []).map((f) => f.id);
    expect(ids).toContain(id);
  }, 60_000);
});

describe("one JVM running social IdP cases between other contexts' cases", () => {
  let runner: RhinoRunner;
  beforeAll(async () => { runner = await RhinoRunner.spawn(); }, 60_000);
  afterAll(async () => { await runner?.close(); }, 30_000);

  // Everything the social IdP seed binds, deletes, rebinds or installs.
  const surface = "JSON.stringify([typeof openidm, typeof httpClient, typeof logger.getName, String(logger.getName()), "
    + "typeof rawProfile, typeof selectedIdp, typeof sharedState, typeof callbacks, typeof nodeState.keys, "
    + "String(nodeState.keys()), typeof org.forgerock.json.JsonValue, typeof org.forgerock.json.JsonValue.json])";
  const nodeProbe = async () => {
    const { effects } = await runCase(runner, defineCase({
      name: "isolation decision node",
      script: `nodeState.putShared("out", ${surface}); action.goTo("true");`,
      given: { sharedState: { seeded: "x" } },
      expect: { outcome: "true", allowUndeclared: { sharedState: true } },
    }));
    return effects.sharedState.final.out;
  };
  const socialCase = () => runCase(runner, defineCase({
    name: "isolation social",
    context: "SOCIAL_IDP_PROFILE_TRANSFORMATION",
    script: `({ a: "b" });`,
    expect: { socialIdp: { profile: { a: "b" } } },
  }));

  it("leaves a decision node exactly as a fresh runner sees it", async () => {
    const before = await nodeProbe();
    await socialCase();
    expect(await nodeProbe()).toBe(before);
  }, 60_000);
});
