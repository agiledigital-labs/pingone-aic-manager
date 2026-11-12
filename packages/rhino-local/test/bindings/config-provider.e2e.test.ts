/**
 * The config provider node context on the real JVM, compared with live
 * captures. surface.script.js ran unchanged as a ConfigProviderNode's script
 * on a sandbox tenant (2026-11-10), in a journey of a decision
 * node seeding the node state, the ConfigProviderNode wrapping a MessageNode,
 * and a decision node reading back. `surface.<engine>.json` holds the probe's
 * record from the request that reached the node (`firstRun`, carried out as
 * the MessageNode's message), and what the readout node found in the node
 * state. `returns.json` holds one-line scripts that pin what AM does with
 * each kind of `config`: the message and buttons the MessageNode showed, or
 * HTTP 500. Here the same scripts run in the emulator, and every observable
 * must agree.
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

const dir = join(packageRoot, "test", "fixtures", "config-provider");
const SURFACE = readFileSync(join(dir, "surface.script.js"), "utf8");

interface Capture {
  http: number[];
  firstRun: Record<string, JsonValue>;
  readout: { cfgPut: string; cfgT: string; keys: string[]; zzStr: string };
}
const capture = (engine: Engine) => JSON.parse(readFileSync(join(dir, `surface.${engine}.json`), "utf8")) as Capture;

interface Returned {
  script: string;
  http: number;
  message?: string;
  options?: string[];
}
const RETURNS = JSON.parse(readFileSync(join(dir, "returns.json"), "utf8")) as Record<
  string,
  Partial<Record<Engine, Returned>>
>;
delete RETURNS._note;

// What the seed decision node put in the node state before the config
// provider ran (seed.js in the probe tooling).
const SHARED = { zzStr: "hello", zzNum: 42, zzObj: { a: 1, b: [1, 2], c: { d: "e" } }, zzBool: true };
// Keys AM's journey put there itself. The probe lists them; their
// values were not read, so these are placeholders.
const AM_OWN = { authLevel: 0, maxAuthenticationSessionDuration: 120, realm: "/alpha", username: "zzemuC4nobody" };
const TRANSIENT = { zzT: "tval" };
const placeholders = (count: number) => {
  const out: Record<string, string[]> = {};
  for (let i = 0; i < count; i += 1) out[`x-placeholder-${i}`] = ["v"];
  return out;
};
const given = (engine: Engine) => {
  const loggerName = capture(engine).firstRun.loggerName as string;
  return {
    engine,
    realm: "/alpha",
    scriptName: "zzemuC4-cfg-ng",
    loggerScriptId: /\.([0-9a-f-]{36})\./.exec(loggerName)![1]!,
    sharedState: { ...AM_OWN, ...SHARED },
    transientState: TRANSIENT,
    // Four parameters and twenty-one headers reached the script; the probe
    // counts them and reads these two.
    requestParameters: { zzq: ["v1"], ...placeholders(3) },
    requestHeaders: { "x-zzemu-probe": ["h1"], ...placeholders(20) },
    cookieName: "rhino-local-cookie",
  };
};

/**
 * How the MessageNode under the probe rendered a config value it read as
 * text, as returns.json observed it: a string as itself, a number as a Java
 * double, a boolean as "true"/"false", null as "null", an object as
 * "[object Object]". Test-only: the wrapped node is not emulated, and the
 * map the emulator records is what is under test here.
 */
const rendered = (v: JsonValue | undefined): string => {
  if (v === null || v === undefined) return "null";
  if (typeof v === "number") return Number.isInteger(v) ? `${v}.0` : String(v);
  if (typeof v === "object") return "[object Object]";
  return String(v);
};
const localised = (v: JsonValue | undefined, fallback: string) => {
  if (v === undefined) return fallback;
  if (v !== null && typeof v === "object" && !Array.isArray(v)) return rendered((v as JsonObject).en);
  return rendered(v);
};

describe("config provider node matches the live capture", () => {
  const engine = "next-gen" as const;
  let runner: RhinoRunner;
  beforeAll(async () => { runner = await RhinoRunner.spawn(); }, 60_000);
  afterAll(async () => { await runner?.close(); }, 30_000);

  const run = (name: string, source: string) =>
    runCase(runner, defineCase({
      name: `${name} ${engine}`,
      context: "CONFIG_PROVIDER_NODE",
      script: source,
      given: given(engine),
      expect: { allowUndeclared: { configProvider: true, sharedState: true, transientState: true, logs: true } },
    }));

  it("surface: the probe's record and the node state", async () => {
    const live = capture(engine);
    expect(live.http).toEqual([200, 200, 200]);
    const { effects, verdict } = await run("surface", SURFACE);
    expect(verdict.pass, verdict.summary).toBe(true);
    const cfg = effects.configProvider!;
    expect(cfg.refusal).toBeNull();
    const message = (cfg.config!.message as JsonObject).en as string;
    const record = JSON.parse(message) as Record<string, JsonValue>;
    const want = { ...live.firstRun };
    // The script context object's error names AM's own script id and line.
    (want.typeof as JsonObject).context = (record.typeof as JsonObject).context!;
    // for...in over idRepository and journey lists their Java members on
    // AIC, java.lang.Object's included; the emulator's objects list only
    // their own JS members (docs/rhino-local-gaps.md). journey's own members
    // agree, so only Object's are dropped there.
    delete want.idRepoKeys;
    delete record.idRepoKeys;
    const javaObject = ["class", "equals", "getClass", "hashCode", "notify", "notifyAll", "toString", "wait"];
    want.journeyKeys = (want.journeyKeys as string).split(",").filter((k) => !javaObject.includes(k)).join(",");
    expect(record).toEqual(want);
    // The readout node saw both puts and the probe's own record.
    expect(effects.sharedState.final.zzCfgPut).toBe(live.readout.cfgPut);
    expect(effects.transientState.final.zzCfgT).toBe(live.readout.cfgT);
    expect(typeof effects.sharedState.final.aicprobe).toBe("string");
  }, 60_000);

  const returns = Object.entries(RETURNS).filter(([, byEngine]) => byEngine[engine] !== undefined);
  it.each(returns)("returns.json %s", async (name, byEngine) => {
    const live = byEngine[engine]!;
    if (live.http === 200) {
      const { effects } = await run(name, live.script);
      const cfg = effects.configProvider!;
      expect(cfg.refusal).toBeNull();
      expect(cfg.returned).toBe("config");
      expect(localised(cfg.config!.message, "Default message")).toBe(live.message);
      expect([localised(cfg.config!.messageYes, "Yes"), localised(cfg.config!.messageNo, "No")]).toEqual(live.options);
      return;
    }
    expect(live.http).toBe(500);
    const outcome = await run(name, live.script).then((r) => r, (e: unknown) => e);
    if (outcome instanceof CaseRunError) {
      // A script that throws fails the run, as AM fails the journey.
      expect(/throw |java\.util|callbacksBuilder|action\.|nodeState\.keys/.test(live.script), live.script).toBe(true);
      return;
    }
    const { effects } = outcome as Awaited<ReturnType<typeof run>>;
    expect(effects.configProvider!.returned).toBe("none");
    expect(effects.configProvider!.refusal).toMatch(/Script must set 'config' to a Map/);
  }, 60_000);

  it("a config value nobody measured is a named fault", async () => {
    expectHarnessFault("config-provider-unmeasured/config.zz[0]");
    const error = await run("null in array", "config = { zz: [null] };").then(() => null, (e: unknown) => e);
    expect(error).toBeInstanceOf(CaseRunError);
    const ids = ((error as CaseRunError).response.emulation?.faults ?? []).map((f) => f.id);
    expect(ids).toContain("config-provider-unmeasured/config.zz[0]");
  }, 60_000);

  it("a config that is not a plain object is a named fault", async () => {
    expectHarnessFault("config-provider-unmeasured/config");
    const error = await run("date", "config = new Date(0);").then(() => null, (e: unknown) => e);
    expect(error).toBeInstanceOf(CaseRunError);
    const ids = ((error as CaseRunError).response.emulation?.faults ?? []).map((f) => f.id);
    expect(ids).toContain("config-provider-unmeasured/config");
  }, 60_000);

  // Sol ctx r4: every path to a seeded request value reads one store, and a
  // conversion, argument shape or set member nobody measured is a named fault.
  it("dot access and for...in reach the same request lists as get()", async () => {
    const source = "var p = requestParameters.zzq; var h = requestHeaders[\"X-Zzemu-Probe\"]; var n = 0;"
      + " for (var k in requestParameters) n += 1;"
      + " config = { message: JSON.stringify([p === requestParameters.get(\"zzq\"), String(p),"
      + " Object.prototype.toString.call(p), h === requestHeaders.get(\"x-zzemu-probe\"), String(h), n]) };";
    const { effects } = await run("request paths", source);
    expect(JSON.parse(effects.configProvider!.config!.message as string)).toEqual(
      [true, "[v1]", "[object JavaList]", true, "[h1]", 4],
    );
  }, 60_000);

  const faults: [string, string][] = [
    ["requestParameters.forEach(function () {}); config = {};", "config-provider-unmeasured/requestParameters.forEach()"],
    ["String(requestHeaders); config = {};", "config-provider-unmeasured/String(requestHeaders)"],
  ];
  faults.push(
    [`callbacksBuilder.textOutputCallback("INFO", "x"); config = {};`,
      `config-provider-unmeasured/callbacksBuilder.textOutputCallback(string, string)`],
    ["String(callbacks); config = {};", "config-provider-unmeasured/String(callbacks)"],
    ["String(callbacksBuilder); config = {};", "config-provider-unmeasured/String(callbacksBuilder)"],
    ["String(action); config = {};", "config-provider-unmeasured/String(action)"],
  );
  it.each(faults)("%s is a named fault", async (source, id) => {
    expectHarnessFault(id);
    const error = await run(id, source).then(() => null, (e: unknown) => e);
    expect(error).toBeInstanceOf(CaseRunError);
    const ids = ((error as CaseRunError).response.emulation?.faults ?? []).map((f) => f.id);
    expect(ids).toContain(id);
  }, 60_000);

  it("textOutputCallback with the measured (number, string) shape is accepted", async () => {
    const { effects } = await run("text output", `callbacksBuilder.textOutputCallback(0, "hello"); config = {};`);
    expect(effects.configProvider!.refusal).toBeNull();
  }, 60_000);

  it("an unmeasured callbacksBuilder or action member is a named fault", async () => {
    for (const [source, id] of [
      [`callbacksBuilder.nameCallback("n"); config = {};`, "config-provider-unmeasured/callbacksBuilder.nameCallback()"],
      [`action.withHeader("h"); config = {};`, "config-provider-unmeasured/action.withHeader()"],
    ] as const) {
      expectHarnessFault(id);
      const error = await run(id, source).then(() => null, (e: unknown) => e);
      expect(error).toBeInstanceOf(CaseRunError);
      const ids = ((error as CaseRunError).response.emulation?.faults ?? []).map((f) => f.id);
      expect(ids).toContain(id);
    }
  }, 60_000);
});

describe("one JVM running config provider cases between other contexts' cases", () => {
  let runner: RhinoRunner;
  beforeAll(async () => { runner = await RhinoRunner.spawn(); }, 60_000);
  afterAll(async () => { await runner?.close(); }, 30_000);

  // Everything the config provider seed binds, deletes, rebinds or installs.
  const member = (binding: string, name: string) =>
    `(typeof ${binding} === "undefined" ? "unbound" : typeof ${binding}.${name})`;
  const surface = "JSON.stringify([typeof config, typeof openidm, typeof logger.getName, String(logger.getName()), "
    + `typeof sharedState, ${member("callbacks", "isEmpty")}, ${member("callbacks", "getNameCallbacks")}, `
    + `${member("callbacksBuilder", "nameCallback")}, ${member("action", "withHeader")}, `
    + "typeof nodeState.keys, String(nodeState.keys()), String(nodeState.get(\"seeded\"))])";
  const nodeProbe = async () => {
    const { effects } = await runCase(runner, defineCase({
      name: "isolation decision node",
      script: `nodeState.putShared("out", ${surface}); action.goTo("true");`,
      given: { sharedState: { seeded: "x" } },
      expect: { outcome: "true", allowUndeclared: { sharedState: true } },
    }));
    return effects.sharedState.final.out;
  };
  const configCase = () => runCase(runner, defineCase({
    name: "isolation config provider",
    context: "CONFIG_PROVIDER_NODE",
    script: `var config = { a: "b" };`,
    given: { sharedState: { seeded: "x" } },
    expect: { configProvider: { config: { a: "b" } } },
  }));

  it("leaves a decision node exactly as a fresh runner sees it", async () => {
    const before = await nodeProbe();
    await configCase();
    expect(await nodeProbe()).toBe(before);
  }, 60_000);

  it("a config provider case after another sees no config left over", async () => {
    await configCase();
    const { effects } = await runCase(runner, defineCase({
      name: "leftover",
      context: "CONFIG_PROVIDER_NODE",
      script: `config = { t: typeof config === "undefined" ? "fresh" : "leftover" };`,
      given: { engine: "next-gen" },
      expect: { configProvider: { config: { t: "fresh" } } },
    }));
    expect(effects.configProvider!.config).toEqual({ t: "fresh" });
  }, 60_000);
});
