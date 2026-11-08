/**
 * IMPROVEMENTS #12: project-wide suite defaults. Precedence, lowest first:
 * defaults, the suite's `always`, the case's `.run()` overrides; `defaults:
 * false` opts a suite out; undeclared-ESV checking is not relaxed.
 */
import { describe, expect, it } from "vitest";
import { expectHarnessFault, layerChannels, useLease, withSuiteDefaults } from "../../src/harness/index.ts";
import { mergeChannels } from "../../src/harness/spec.ts";
import type { HttpStub } from "../../src/case/types.ts";
import type { Channels } from "../../src/harness/types.ts";

describe("layerChannels", () => {
  const stub = (url: string): HttpStub => ({ match: { url }, reply: { status: 200 } }) as HttpStub;

  it("merges maps per key, upper layer winning, and keeps undeclared channels undeclared", () => {
    const out = layerChannels(
      { esv: { a: "base", b: "base" }, state: { shared: { s: 1, t: 1 } }, cookieName: "base" },
      { esv: { b: "top" }, state: { shared: { t: 2 } } },
    );
    expect(out).toEqual({ esv: { a: "base", b: "top" }, state: { shared: { s: 1, t: 2 }, transient: {} }, cookieName: "base" });
    expect("session" in out).toBe(false);
    expect("http" in out).toBe(false);
  });

  it("orders stub lists upper layer first, as mergeChannels orders a case before its suite", () => {
    expect(layerChannels({ http: [stub("base")] }, { http: [stub("top")] }).http).toEqual([stub("top"), stub("base")]);
  });

  it("a session declared only in the defaults still asks for one", () => {
    const draft = mergeChannels(layerChannels({ session: {} }, {}), undefined);
    expect(draft.sessionRequested).toBe(true);
  });

  it("is associative with mergeChannels: defaults → suite → case equals one merge of the layered suite", () => {
    const defaults: Channels = { esv: { x: "d", y: "d" }, esvUndeclared: "absent", openidmActions: [] };
    const suite: Channels = { esv: { y: "s" }, esvUndeclared: "error" };
    const kase: Channels = { esv: { x: "c" } };
    expect(mergeChannels(layerChannels(defaults, suite), kase).esv).toEqual({ x: "c", y: "s" });
    expect(mergeChannels(layerChannels(defaults, suite), kase).esvUndeclared).toBe("error");
  });
});

const defineProjectSuite = withSuiteDefaults({
  always: { esv: { "common.url": "default-url", "common.mode": "default-mode" } },
  libraries: { common: 'exports.tag = function () { return "default-lib"; };' },
});
const script = [
  'var c = require("common");',
  'action.goTo(systemEnv.getProperty("esv.common.url") + "|" + systemEnv.getProperty("esv.common.mode") + "|" + c.tag());',
].join("\n");

describe("withSuiteDefaults on Rhino", () => {
  const layered = useLease(defineProjectSuite({
    name: "suite-defaults",
    script,
    outcomes: ["default-url|suite-mode|default-lib", "case-url|suite-mode|default-lib"],
    always: { esv: { "common.mode": "suite-mode" } },
  }));

  it("a suite inherits the defaults and overrides one key", async () => {
    const run = await layered.run().expect({ outcome: "default-url|suite-mode|default-lib" });
    expect(run.verdict.pass, run.verdict.summary).toBe(true);
  });

  it("a case overrides both", async () => {
    const run = await layered.run().esv({ "common.url": "case-url" }).expect({ outcome: "case-url|suite-mode|default-lib" });
    expect(run.verdict.pass, run.verdict.summary).toBe(true);
  });

  const ownLibrary = useLease(defineProjectSuite({
    name: "suite-defaults-library",
    script,
    outcomes: ["default-url|default-mode|suite-lib"],
    libraries: { common: 'exports.tag = function () { return "suite-lib"; };' },
  }));
  it("a suite's library replaces the default library of the same name", async () => {
    const run = await ownLibrary.run().expect({ outcome: "default-url|default-mode|suite-lib" });
    expect(run.verdict.pass, run.verdict.summary).toBe(true);
  });

  const optedOut = useLease(defineProjectSuite({
    name: "suite-defaults-opt-out",
    script: 'action.goTo(String(systemEnv.getProperty("esv.common.url")));',
    outcomes: ["default-url", "null"],
    defaults: false,
  }));
  it("defaults: false opts out, and the undeclared ESV is still refused", async () => {
    expectHarnessFault("fixture/esv");
    await expect(optedOut.run().expect({ outcome: "null" })).rejects.toThrow(/no given\.esv entry for "esv\.common\.url"/);
  });
});
