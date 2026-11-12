/**
 * The OAUTH2_VALIDATE_SCOPE context's case validation and verdict, without a
 * JVM. The e2e conformance against live captures is
 * test/bindings/oauth2-validate-scope.e2e.test.ts.
 */
import { describe, expect, it } from "vitest";
import { aicUnsupportedReason } from "../../src/aic/unsupported.ts";
import { defineCase, judge, validateCase } from "../../src/case/index.ts";
import type { JsonObject, ValidateScopeEffect } from "../../src/case/index.ts";
import { makeEffects } from "./helpers.ts";

const VS = "OAUTH2_VALIDATE_SCOPE" as const;
const vsCase = (expect: JsonObject = {}, given: JsonObject = {}) =>
  ({ name: "vs", script: "x.js", context: VS, given, expect }) as unknown;

function effects(overrides: Partial<ValidateScopeEffect> = {}) {
  const validateScope: ValidateScopeEffect = {
    returned: "scopes", status: 200, granted: ["a", "b"], invalidScope: null, refusal: null, ...overrides,
  };
  return { ...makeEffects({ outcome: null }), validateScope };
}
const declined = (): Partial<ValidateScopeEffect> => ({
  returned: "none", status: 403, granted: null, refusal: "The user has declined authorization",
});
const invalid = (message: string): Partial<ValidateScopeEffect> => ({
  returned: "none", status: 400, granted: null, invalidScope: message, refusal: message,
});

describe("OAUTH2_VALIDATE_SCOPE case validation", () => {
  it("refuses a validate scope key on the decision node and names the selector", () => {
    for (const [section, body] of [
      ["expect", { expect: { outcome: "true", validateScope: {} } }],
      ["allowUndeclared", { expect: { outcome: "true", allowUndeclared: { validateScope: true } } }],
      ["given", { given: { validateScope: {} }, expect: { outcome: "true" } }],
    ] as const) {
      expect(() => validateCase({ name: "n", script: "s", ...body }), section).toThrow(/set case\.context/);
    }
  });

  it("refuses node state, callbacks and other contexts' keys", () => {
    expect(() => validateCase(vsCase({}, { sharedState: {} }))).toThrow(/given\.sharedState is not accepted in context/);
    expect(() => validateCase(vsCase({}, { configProvider: {} }))).toThrow(/is not accepted in context/);
    expect(() => validateCase(vsCase({ callbacks: [] }))).toThrow(/expect\.callbacks is not accepted in context/);
    expect(() => validateCase(vsCase({ accessToken: {} }))).toThrow(/expect\.accessToken is not accepted in context/);
  });

  it("accepts the scope lists, client and request", () => {
    {
      const engine = "next-gen" as const;
      const kase = defineCase({
        name: "ok",
        script: "x.js",
        context: VS,
        given: {
          engine,
          validateScope: { requested: ["a"], allowed: ["a", "b"], available: ["a", "b"], defaults: ["b"] },
          client: { clientId: "client-a" },
          request: { params: { scope: ["a"] } },
        },
        expect: { validateScope: { granted: ["a"] } },
      });
      expect(kase.expect.validateScope).toEqual({ granted: ["a"] });
    }
  });

  it("checks the seed and the expectation by shape", () => {
    expect(() => validateCase(vsCase({}, { validateScope: { requested: "a" } }))).toThrow(/requested/);
    expect(() => validateCase(vsCase({}, { validateScope: { scopes: [] } }))).toThrow(/scopes/);
    expect(() => validateCase(vsCase({ validateScope: { accepted: "no" } }))).toThrow(/accepted must be a boolean/);
    expect(() => validateCase(vsCase({ validateScope: { granted: ["a"], invalidScope: "x" } }))).toThrow(/cannot be declared with a refusal/);
    expect(() => validateCase(vsCase({ validateScope: { granted: ["a"], accepted: false } }))).toThrow(/cannot be declared with a refusal/);
  });

  it("runs locally only", () => {
    expect(aicUnsupportedReason(validateCase(vsCase()) as never)).toMatch(/another script context runs locally only/);
  });
});

describe("OAUTH2_VALIDATE_SCOPE verdict", () => {
  it("fails a case whose run recorded no result", () => {
    const verdict = judge(vsCase(), makeEffects({ outcome: null }));
    expect(verdict.mismatches.map((m) => m.path)).toEqual(["validateScope"]);
  });

  it("compares the grant as a set, so order does not matter and a missing or extra scope does", () => {
    expect(judge(vsCase({ validateScope: { granted: ["b", "a"] } }), effects()).pass).toBe(true);
    expect(judge(vsCase({ validateScope: { granted: ["a"] } }), effects()).mismatches.map((m) => m.path)).toEqual(["granted"]);
    expect(judge(vsCase({ validateScope: { granted: ["a", "b", "c"] } }), effects()).pass).toBe(false);
  });

  it("calls an undeclared grant a mismatch unless allowUndeclared.validateScope", () => {
    expect(judge(vsCase(), effects()).mismatches.map((m) => m.path)).toEqual(["granted"]);
    expect(judge(vsCase({ allowUndeclared: { validateScope: true } }), effects()).pass).toBe(true);
  });

  it("expects a token unless the case declares a refusal", () => {
    const failed = judge(vsCase(), effects(declined()));
    expect(failed.mismatches.map((m) => m.path)).toEqual(["accepted"]);
    expect(failed.mismatches[0]!.message).toMatch(/403: The user has declined authorization/);
    expect(judge(vsCase({ validateScope: { accepted: false } }), effects(declined())).pass).toBe(true);
    expect(judge(vsCase({ validateScope: { accepted: false } }), effects()).mismatches.map((m) => m.path)).toEqual(["accepted"]);
  });

  it("an invalidScope declaration implies a refusal, and its message is compared", () => {
    expect(judge(vsCase({ validateScope: { invalidScope: "nope" } }), effects(invalid("nope"))).pass).toBe(true);
    const other = judge(vsCase({ validateScope: { invalidScope: "nope" } }), effects(invalid("other")));
    expect(other.mismatches.map((m) => m.path)).toEqual(["invalidScope"]);
    // A 403 decline is a refusal but not the invalid-scope denial.
    const declinedInstead = judge(vsCase({ validateScope: { invalidScope: "nope" } }), effects(declined()));
    expect(declinedInstead.mismatches.map((m) => m.path)).toEqual(["invalidScope"]);
  });
});
