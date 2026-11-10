/**
 * The OIDC_CLAIMS context's case validation and verdict, without a JVM. The
 * e2e conformance against live captures is
 * test/bindings/oidc-claims.e2e.test.ts.
 */
import { describe, expect, it } from "vitest";
import { aicUnsupportedReason } from "../../src/aic/unsupported.ts";
import { defineCase, judge, validateCase } from "../../src/case/index.ts";
import type { JsonObject, JsonValue, OidcClaimsEffect } from "../../src/case/index.ts";
import { makeEffects } from "./helpers.ts";

const OIDC = "OIDC_CLAIMS" as const;
const oidcCase = (expect: JsonObject = {}, given: JsonObject = {}) =>
  ({ name: "oidc", script: "x.js", context: OIDC, given, expect }) as unknown;

function effects(overrides: Partial<OidcClaimsEffect> = {}) {
  const oidc: OidcClaimsEffect = {
    returned: "claims",
    userinfo: {},
    idToken: { sub: "u" },
    compositeScopes: null,
    refusal: null,
    ...overrides,
  };
  return { ...makeEffects({ outcome: null }), oidc };
}

describe("OIDC_CLAIMS case validation", () => {
  it("refuses an OIDC key on the decision node and names the selector", () => {
    for (const [section, body] of [
      ["given", { given: { oidc: {} }, expect: { outcome: "true" } }],
      ["expect", { expect: { outcome: "true", oidc: {} } }],
      ["allowUndeclared", { expect: { outcome: "true", allowUndeclared: { oidc: true } } }],
    ] as const) {
      expect(() => validateCase({ name: "n", script: "s", ...body }), section).toThrow(/set case\.context/);
    }
  });

  it("refuses decision-node and access-token keys in the OIDC context", () => {
    expect(() => validateCase(oidcCase({}, { nodeState: {} }))).toThrow(/given\.nodeState is not accepted in context/);
    expect(() => validateCase(oidcCase({}, { accessToken: {} }))).toThrow(/given\.accessToken is not accepted in context/);
    expect(() => validateCase(oidcCase({ callbacks: [] }))).toThrow(/expect\.callbacks is not accepted in context/);
    expect(() => validateCase(oidcCase({ accessToken: {} }))).toThrow(/expect\.accessToken is not accepted in context/);
    expect(() => validateCase(oidcCase({ allowUndeclared: { accessToken: true } }))).toThrow(/accessToken/);
  });

  it("checks the seed and the expectation by shape", () => {
    expect(() => validateCase(oidcCase({}, { oidc: { sub: 1 } }))).toThrow(/given\.oidc\.sub must be a string/);
    expect(() => validateCase(oidcCase({}, { oidc: { claimsLocales: "en" } }))).toThrow(/claimsLocales must be an array of strings/);
    expect(() => validateCase(oidcCase({}, { oidc: { zz: 1 } }))).toThrow(/zz/);
    expect(() => validateCase(oidcCase({ oidc: { issued: "no" } }))).toThrow(/issued must be a boolean/);
    expect(() => validateCase(oidcCase({ oidc: { claims: {} } }))).toThrow(/claims/);
    const kase = defineCase({
      name: "ok",
      script: "x.js",
      context: OIDC,
      given: { engine: "legacy", oidc: { sub: "s", claimsLocales: ["en"] }, scopes: ["openid"] },
      expect: { oidc: { userinfo: { a: "b" }, issued: true } },
    });
    expect(kase.given.oidc).toEqual({ sub: "s", claimsLocales: ["en"] });
  });

  it("runs locally only", () => {
    expect(aicUnsupportedReason(validateCase(oidcCase()) as never)).toMatch(/another script context runs locally only/);
  });
});

describe("OIDC_CLAIMS verdict", () => {
  it("fails a case whose run recorded no OIDC result", () => {
    const verdict = judge(oidcCase(), makeEffects({ outcome: null }));
    expect(verdict.pass).toBe(false);
    expect(verdict.mismatches.map((m) => m.path)).toEqual(["oidc"]);
  });

  it("passes a declared userinfo body and fails a different one", () => {
    expect(judge(oidcCase({ oidc: { userinfo: { a: "b" } } }), effects({ userinfo: { a: "b" } })).pass).toBe(true);
    const wrong = judge(oidcCase({ oidc: { userinfo: { a: "c" } } }), effects({ userinfo: { a: "b" } }));
    expect(wrong.mismatches.map((m) => m.path)).toEqual(["userinfo"]);
  });

  it("calls undeclared claims a mismatch unless allowUndeclared.oidc", () => {
    expect(judge(oidcCase(), effects({ userinfo: { a: "b" } })).mismatches.map((m) => m.path)).toEqual(["userinfo"]);
    expect(judge(oidcCase({ allowUndeclared: { oidc: true } }), effects({ userinfo: { a: "b" } })).pass).toBe(true);
    // No claims at all is nothing to declare.
    expect(judge(oidcCase(), effects()).pass).toBe(true);
  });

  it("never calls the id token undeclared: it is derived from the userinfo values and the seed", () => {
    expect(judge(oidcCase(), effects({ idToken: { sub: "u", extra: 1 } })).pass).toBe(true);
    const declared = judge(oidcCase({ oidc: { idToken: { sub: "v" } } }), effects());
    expect(declared.mismatches.map((m) => m.path)).toEqual(["idToken"]);
  });

  it("treats null and an empty map of composite scopes alike, and a populated one as undeclared", () => {
    const cases: [JsonValue, boolean][] = [[null, true], [{}, true], [{ s: ["a"] }, false], [[], false]];
    for (const [compositeScopes, pass] of cases) {
      expect(judge(oidcCase(), effects({ compositeScopes })).pass, JSON.stringify(compositeScopes)).toBe(pass);
    }
  });

  it("expects the token request to succeed unless issued: false", () => {
    const refused = effects({ returned: "none", refusal: "Cannot cast java.util.LinkedHashMap to x", userinfo: { a: "b" } });
    const failed = judge(oidcCase(), refused);
    // Only the refusal is reported; a request that failed issued no claims.
    expect(failed.mismatches.map((m) => m.path)).toEqual(["issued"]);
    expect(failed.mismatches[0]!.message).toMatch(/Cannot cast/);
    expect(judge(oidcCase({ oidc: { issued: false } }), refused).pass).toBe(true);
    const notRefused = judge(oidcCase({ oidc: { issued: false } }), effects());
    expect(notRefused.mismatches.map((m) => m.path)).toEqual(["issued"]);
  });
});
