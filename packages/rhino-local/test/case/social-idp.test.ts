/**
 * The SOCIAL_IDP_PROFILE_TRANSFORMATION context's case validation and
 * verdict, without a JVM. The e2e conformance against live captures is
 * test/bindings/social-idp.e2e.test.ts.
 */
import { describe, expect, it } from "vitest";
import { aicUnsupportedReason } from "../../src/aic/unsupported.ts";
import { defineCase, judge, validateCase } from "../../src/case/index.ts";
import type { JsonObject, SocialIdpEffect } from "../../src/case/index.ts";
import { makeEffects } from "./helpers.ts";

const SOCIAL = "SOCIAL_IDP_PROFILE_TRANSFORMATION" as const;
const socialCase = (expect: JsonObject = {}, given: JsonObject = {}) =>
  ({ name: "social", script: "x.js", context: SOCIAL, given, expect }) as unknown;

function effects(overrides: Partial<SocialIdpEffect> = {}, base = makeEffects({ outcome: null })) {
  const socialIdp: SocialIdpEffect = { returned: "profile", profile: { id: "u" }, refusal: null, ...overrides };
  return { ...base, socialIdp };
}
const refused = (): Partial<SocialIdpEffect> => ({
  returned: "none",
  profile: null,
  refusal: "the transformation evaluated to null; AM fails the journey (401 Login failure)",
});

describe("SOCIAL_IDP_PROFILE_TRANSFORMATION case validation", () => {
  it("refuses a social IdP key on the decision node and names the selector", () => {
    for (const [section, body] of [
      ["given", { given: { socialIdp: {} }, expect: { outcome: "true" } }],
      ["expect", { expect: { outcome: "true", socialIdp: {} } }],
      ["allowUndeclared", { expect: { outcome: "true", allowUndeclared: { socialIdp: true } } }],
    ] as const) {
      expect(() => validateCase({ name: "n", script: "s", ...body }), section).toThrow(/set case\.context/);
    }
  });

  it("refuses keys of other contexts, and decision-node keys the context does not bind", () => {
    expect(() => validateCase(socialCase({}, { oidc: {} }))).toThrow(/given\.oidc is not accepted in context/);
    expect(() => validateCase(socialCase({}, { secureState: {} }))).toThrow(/given\.secureState is not accepted in context/);
    expect(() => validateCase(socialCase({}, { existingSession: {} }))).toThrow(/given\.existingSession is not accepted/);
    expect(() => validateCase(socialCase({ callbacks: [] }))).toThrow(/expect\.callbacks is not accepted in context/);
    expect(() => validateCase(socialCase({ oidc: {} }))).toThrow(/expect\.oidc is not accepted in context/);
    expect(() => validateCase(socialCase({ allowUndeclared: { callbacks: true } }))).toThrow(/callbacks/);
  });

  it("accepts the node state, which the context binds", () => {
    const kase = defineCase({
      name: "ok",
      script: "x.js",
      context: SOCIAL,
      given: { engine: "next-gen", sharedState: { a: "b" }, socialIdp: { rawProfile: { sub: "s" }, selectedIdp: "idp" } },
      expect: { socialIdp: { profile: { id: "s" }, accepted: true }, sharedState: { added: { c: "d" } } },
    });
    expect(kase.given.socialIdp).toEqual({ rawProfile: { sub: "s" }, selectedIdp: "idp" });
    expect(kase.expect.socialIdp).toEqual({ profile: { id: "s" }, accepted: true });
  });

  it("checks the seed and the expectation by shape", () => {
    expect(() => validateCase(socialCase({}, { socialIdp: { selectedIdp: 1 } }))).toThrow(/selectedIdp must be a string/);
    expect(() => validateCase(socialCase({}, { socialIdp: { rawProfile: [] } }))).toThrow(/rawProfile/);
    expect(() => validateCase(socialCase({}, { socialIdp: { zz: 1 } }))).toThrow(/zz/);
    expect(() => validateCase(socialCase({ socialIdp: { accepted: "no" } }))).toThrow(/accepted must be a boolean/);
    expect(() => validateCase(socialCase({ socialIdp: { normalizedProfile: {} } }))).toThrow(/normalizedProfile/);
  });

  it("runs locally only", () => {
    expect(aicUnsupportedReason(validateCase(socialCase()) as never)).toMatch(/another script context runs locally only/);
  });
});

describe("SOCIAL_IDP_PROFILE_TRANSFORMATION verdict", () => {
  it("fails a case whose run recorded no result", () => {
    const verdict = judge(socialCase(), makeEffects({ outcome: null }));
    expect(verdict.pass).toBe(false);
    expect(verdict.mismatches.map((m) => m.path)).toEqual(["socialIdp"]);
  });

  it("passes a declared profile and fails a different one", () => {
    expect(judge(socialCase({ socialIdp: { profile: { id: "u" } } }), effects()).pass).toBe(true);
    const wrong = judge(socialCase({ socialIdp: { profile: { id: "v" } } }), effects());
    expect(wrong.mismatches.map((m) => m.path)).toEqual(["profile"]);
  });

  it("calls an undeclared profile a mismatch unless allowUndeclared.socialIdp", () => {
    expect(judge(socialCase(), effects()).mismatches.map((m) => m.path)).toEqual(["profile"]);
    expect(judge(socialCase({ allowUndeclared: { socialIdp: true } }), effects()).pass).toBe(true);
  });

  it("expects AM to accept the profile unless accepted: false", () => {
    const failed = judge(socialCase(), effects(refused()));
    // Only the refusal is reported; a failed journey has no profile to declare.
    expect(failed.mismatches.map((m) => m.path)).toEqual(["accepted"]);
    expect(failed.mismatches[0]!.message).toMatch(/AM fails the journey/);
    expect(judge(socialCase({ socialIdp: { accepted: false } }), effects(refused())).pass).toBe(true);
    const notRefused = judge(socialCase({ socialIdp: { accepted: false, profile: { id: "u" } } }), effects());
    expect(notRefused.mismatches.map((m) => m.path)).toEqual(["accepted"]);
  });

  it("judges node-state writes like the decision node's, fail-closed", () => {
    const base = makeEffects({ outcome: null, sharedState: { initial: {}, final: { k: "v" } } });
    const undeclared = judge(socialCase({ socialIdp: { profile: { id: "u" } } }), effects({}, base));
    expect(undeclared.pass).toBe(false);
    expect(undeclared.mismatches.map((m) => m.channel)).toContain("sharedState");
    const declared = judge(
      socialCase({ socialIdp: { profile: { id: "u" } }, sharedState: { added: { k: "v" } } }),
      effects({}, base)
    );
    expect(declared.pass, declared.summary).toBe(true);
  });
});
