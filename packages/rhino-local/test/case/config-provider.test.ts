/**
 * The CONFIG_PROVIDER_NODE context's case validation and verdict, without a
 * JVM. The e2e conformance against live captures is
 * test/bindings/config-provider.e2e.test.ts.
 */
import { describe, expect, it } from "vitest";
import { aicUnsupportedReason } from "../../src/aic/unsupported.ts";
import { defineCase, judge, validateCase } from "../../src/case/index.ts";
import type { ConfigProviderEffect, JsonObject } from "../../src/case/index.ts";
import { makeEffects } from "./helpers.ts";

const CONFIG = "CONFIG_PROVIDER_NODE" as const;
const configCase = (expect: JsonObject = {}, given: JsonObject = {}) =>
  ({ name: "config", script: "x.js", context: CONFIG, given, expect }) as unknown;

function effects(overrides: Partial<ConfigProviderEffect> = {}, base = makeEffects({ outcome: null })) {
  const configProvider: ConfigProviderEffect = { returned: "config", config: { message: "m" }, refusal: null, ...overrides };
  return { ...base, configProvider };
}
const refused = (): Partial<ConfigProviderEffect> => ({
  returned: "none",
  config: null,
  refusal: "config is never assigned; AM fails the journey with \"Script must set 'config' to a Map.\" (HTTP 500)",
});

describe("CONFIG_PROVIDER_NODE case validation", () => {
  it("refuses a config provider key on the decision node and names the selector", () => {
    for (const [section, body] of [
      ["expect", { expect: { outcome: "true", configProvider: {} } }],
      ["allowUndeclared", { expect: { outcome: "true", allowUndeclared: { configProvider: true } } }],
    ] as const) {
      expect(() => validateCase({ name: "n", script: "s", ...body }), section).toThrow(/set case\.context/);
    }
  });

  it("refuses keys of other contexts, and decision-node keys the context does not bind", () => {
    expect(() => validateCase(configCase({}, { socialIdp: {} }))).toThrow(/given\.socialIdp is not accepted in context/);
    expect(() => validateCase(configCase({}, { secureState: {} }))).toThrow(/given\.secureState is not accepted in context/);
    expect(() => validateCase(configCase({}, { existingSession: {} }))).toThrow(/given\.existingSession is not accepted/);
    expect(() => validateCase(configCase({}, { callbacks: [] }))).toThrow(/given\.callbacks is not accepted/);
    expect(() => validateCase(configCase({ callbacks: [] }))).toThrow(/expect\.callbacks is not accepted in context/);
    expect(() => validateCase(configCase({ socialIdp: {} }))).toThrow(/expect\.socialIdp is not accepted in context/);
    expect(() => validateCase(configCase({ allowUndeclared: { callbacks: true } }))).toThrow(/callbacks/);
  });

  it("accepts the node state and request, which the context binds", () => {
    {
      const engine = "next-gen" as const;
      const kase = defineCase({
        name: "ok",
        script: "x.js",
        context: CONFIG,
        given: { engine, sharedState: { a: "b" }, transientState: { t: "v" }, requestHeaders: { h: ["v"] } },
        expect: { configProvider: { config: { message: "m" }, accepted: true }, sharedState: { added: { c: "d" } } },
      });
      expect(kase.expect.configProvider).toEqual({ config: { message: "m" }, accepted: true });
    }
  });

  it("checks the expectation by shape", () => {
    expect(() => validateCase(configCase({ configProvider: { accepted: "no" } }))).toThrow(/accepted must be a boolean/);
    expect(() => validateCase(configCase({ configProvider: { map: {} } }))).toThrow(/map/);
  });

  it("runs locally only", () => {
    expect(aicUnsupportedReason(validateCase(configCase()) as never)).toMatch(/another script context runs locally only/);
  });
});

describe("CONFIG_PROVIDER_NODE verdict", () => {
  it("fails a case whose run recorded no result", () => {
    const verdict = judge(configCase(), makeEffects({ outcome: null }));
    expect(verdict.pass).toBe(false);
    expect(verdict.mismatches.map((m) => m.path)).toEqual(["configProvider"]);
  });

  it("passes a declared config and fails a different one", () => {
    expect(judge(configCase({ configProvider: { config: { message: "m" } } }), effects()).pass).toBe(true);
    const wrong = judge(configCase({ configProvider: { config: { message: "n" } } }), effects());
    expect(wrong.mismatches.map((m) => m.path)).toEqual(["config"]);
  });

  it("calls an undeclared config a mismatch unless allowUndeclared.configProvider", () => {
    expect(judge(configCase(), effects()).mismatches.map((m) => m.path)).toEqual(["config"]);
    expect(judge(configCase({ allowUndeclared: { configProvider: true } }), effects()).pass).toBe(true);
  });

  it("expects AM to accept the config unless accepted: false", () => {
    const failed = judge(configCase(), effects(refused()));
    // Only the refusal is reported; a failed journey has no config to declare.
    expect(failed.mismatches.map((m) => m.path)).toEqual(["accepted"]);
    expect(failed.mismatches[0]!.message).toMatch(/Script must set 'config' to a Map/);
    expect(judge(configCase({ configProvider: { accepted: false } }), effects(refused())).pass).toBe(true);
    const notRefused = judge(configCase({ configProvider: { accepted: false, config: { message: "m" } } }), effects());
    expect(notRefused.mismatches.map((m) => m.path)).toEqual(["accepted"]);
  });

  it("judges node-state writes like the decision node's, fail-closed", () => {
    const base = makeEffects({ outcome: null, sharedState: { initial: {}, final: { k: "v" } } });
    const undeclared = judge(configCase({ configProvider: { config: { message: "m" } } }), effects({}, base));
    expect(undeclared.pass).toBe(false);
    expect(undeclared.mismatches.map((m) => m.channel)).toContain("sharedState");
    const declared = judge(
      configCase({ configProvider: { config: { message: "m" } }, sharedState: { added: { k: "v" } } }),
      effects({}, base)
    );
    expect(declared.pass, declared.summary).toBe(true);
  });
});
