/**
 * The case-API context selector (src/case/context.ts) and the access-token
 * verdict, without a JVM. The e2e conformance against live captures is
 * test/bindings/oauth2-access-token.e2e.test.ts.
 */
import { describe, expect, it } from "vitest";
import { aicUnsupportedReason } from "../../src/aic/unsupported.ts";
import { defineCase, judge, validateCase } from "../../src/case/index.ts";
import type { AccessTokenEffect, AccessTokenState, JsonObject } from "../../src/case/index.ts";
import { makeCase, makeEffects } from "./helpers.ts";

const ATM = "OAUTH2_ACCESS_TOKEN_MODIFICATION" as const;

function tokenState(overrides: Partial<AccessTokenState> = {}): AccessTokenState {
  return {
    fields: {},
    responseFields: {},
    scope: ["p"],
    expiryTime: 1_000,
    nonce: null,
    authLevel: null,
    claims: null,
    act: null,
    mayAct: null,
    permissions: null,
    ...overrides,
  };
}

function tokenEffects(final: Partial<AccessTokenState>, refusal: string | null = null) {
  const accessToken: AccessTokenEffect = { initial: tokenState(), final: tokenState(final), refusal };
  return { ...makeEffects({ outcome: null }), accessToken };
}

const atmCase = (expect: JsonObject = {}, given: JsonObject = {}) =>
  ({ name: "atm", script: "x.js", context: ATM, given, expect }) as unknown;

describe("case.context selector", () => {
  it("leaves a case with no context exactly as before: no context key, decision-node rules", () => {
    const kase = makeCase({ expect: { outcome: "true" } });
    expect("context" in kase).toBe(false);
    expect(validateCase({ name: "n", script: "s", context: "SCRIPTED_DECISION_NODE", expect: { outcome: "true" } }))
      .toMatchObject({ context: "SCRIPTED_DECISION_NODE" });
  });

  it("refuses a token-context key on the decision node and names the selector", () => {
    for (const [section, body] of [
      ["given", { given: { accessToken: {} }, expect: { outcome: "true" } }],
      ["given", { given: { identity: {} }, expect: { outcome: "true" } }],
      ["expect", { expect: { outcome: "true", accessToken: {} } }],
      ["allowUndeclared", { expect: { outcome: "true", allowUndeclared: { accessToken: true } } }],
    ] as const) {
      expect(() => validateCase({ name: "n", script: "s", ...body }), section).toThrow(/set case\.context/);
    }
  });

  it("refuses a decision-node key in the token context", () => {
    expect(() => validateCase(atmCase({}, { nodeState: {} }))).toThrow(/given\.nodeState is not accepted in context/);
    expect(() => validateCase(atmCase({ callbacks: [] }))).toThrow(/expect\.callbacks is not accepted in context/);
    expect(() => validateCase(atmCase({ allowUndeclared: { sharedState: true } })))
      .toThrow(/allowUndeclared\.sharedState is not accepted/);
  });

  it("refuses an outcome in a context whose script decides none, and accepts none or null", () => {
    expect(() => validateCase(atmCase({ outcome: "true" }))).toThrow(/outcome/);
    expect(validateCase(atmCase({})).expect.outcome).toBeNull();
    expect(validateCase(atmCase({ outcome: null })).expect.outcome).toBeNull();
    expect(() => validateCase({ ...(atmCase() as object), outcomes: ["true"] })).toThrow(/outcomes/);
  });

  it("refuses an unknown context, with a hint for AM's _NEXT_GEN spelling", () => {
    expect(() => validateCase({ name: "n", script: "s", context: "NOPE", expect: { outcome: "true" } }))
      .toThrow(/must be one of/);
    expect(() => validateCase({ name: "n", script: "s", context: `${ATM}_NEXT_GEN`, expect: {} }))
      .toThrow(/drop the _NEXT_GEN suffix/);
  });

  it("validates the token seeds by shape", () => {
    expect(() => validateCase(atmCase({}, { accessToken: { scope: "p" } }))).toThrow(/scope must be an array of strings/);
    expect(() => validateCase(atmCase({}, { accessToken: { zz: 1 } }))).toThrow(/zz/);
    expect(() => validateCase(atmCase({}, { identity: { exists: "yes" } }))).toThrow(/exists must be a boolean/);
    expect(() => validateCase(atmCase({ accessToken: { issued: "no" } }))).toThrow(/issued must be a boolean/);
    const kase = defineCase({
      name: "typed",
      script: "s",
      context: ATM,
      given: { engine: "next-gen", accessToken: { clientId: "c", scope: ["a", "b"] }, client: { allowedScopes: ["a"] } },
      expect: { accessToken: { scope: ["a"] } },
    });
    expect(kase.given.accessToken).toEqual({ clientId: "c", scope: ["a", "b"] });
  });

  it("keeps a token-context case off the AIC lane", () => {
    expect(aicUnsupportedReason(validateCase(atmCase()))).toMatch(/runs locally only/);
    expect(aicUnsupportedReason(makeCase())).toBeUndefined();
  });
});

describe("access-token verdict", () => {
  it("passes a declared field change and fails an undeclared one", () => {
    const declared = judge(atmCase({ accessToken: { fields: { added: { zz: 1 } } } }), tokenEffects({ fields: { zz: 1 } }));
    expect(declared.pass, declared.summary).toBe(true);
    const undeclared = judge(atmCase(), tokenEffects({ fields: { zz: 1 } }));
    expect(undeclared.pass).toBe(false);
    expect(undeclared.mismatches.map((m) => m.path)).toContain("fields.zz");
    const allowed = judge(atmCase({ allowUndeclared: { accessToken: true } }), tokenEffects({ fields: { zz: 1 } }));
    expect(allowed.pass, allowed.summary).toBe(true);
  });

  it("compares scope as a set, and calls an undeclared scope change a mismatch", () => {
    expect(judge(atmCase({ accessToken: { scope: ["b", "a"] } }), tokenEffects({ scope: ["a", "b"] })).pass).toBe(true);
    expect(judge(atmCase({ accessToken: { scope: ["a"] } }), tokenEffects({ scope: ["a", "b"] })).pass).toBe(false);
    // Same members in another order is no change at all.
    expect(judge(atmCase(), tokenEffects({ scope: ["p"] })).pass).toBe(true);
    expect(judge(atmCase(), tokenEffects({ scope: ["p", "q"] })).mismatches.map((m) => m.path)).toEqual(["scope"]);
  });

  it("refuses a repeated scope, so [read, read] cannot pass for [read, write] (Sol ctx r1 P1-1)", () => {
    expect(() => validateCase(atmCase({ accessToken: { scope: ["read", "read"] } }))).toThrow(/lists "read" twice/);
    expect(() => validateCase(atmCase({}, { accessToken: { scope: ["read", "read"] } }))).toThrow(/lists "read" twice/);
    // The comparison itself is two-way, whatever reaches it: a harvested
    // scope with a repeat still differs from a set that lacks a member.
    const repeated = tokenEffects({ scope: ["read", "read"] });
    expect(judge(atmCase({ accessToken: { scope: ["read", "write"] } }), repeated).pass).toBe(false);
    expect(judge(atmCase({ accessToken: { scope: ["read"] } }), repeated).pass).toBe(true);
  });

  it("records an undeclared own __proto__ field as a change in both token buckets (Sol ctx r1 P1-2)", () => {
    const proto = JSON.parse('{"__proto__": {"x": 1}}') as JsonObject;
    for (const bucket of ["fields", "responseFields"] as const) {
      const verdict = judge(atmCase(), tokenEffects({ [bucket]: proto }));
      expect(verdict.pass, bucket).toBe(false);
      expect(verdict.mismatches.map((m) => m.path)).toContain(`${bucket}.__proto__`);
      const declared = JSON.parse(`{"accessToken": {"${bucket}": {"added": {"__proto__": {"x": 1}}}}}`) as JsonObject;
      expect(judge(atmCase(declared), tokenEffects({ [bucket]: proto })).pass, bucket).toBe(true);
    }
  });

  it("keeps __proto__ as an own fixture key, and validating twice gives the same case (Sol ctx r1 P2-4)", () => {
    const given = JSON.parse(`{
      "identity": { "attributes": { "__proto__": ["g"] } },
      "request": { "params": { "__proto__": ["p"] }, "headers": { "__proto__": ["h"] } },
      "client": { "customProperties": { "__proto__": "c" } }
    }`) as JsonObject;
    const once = validateCase(atmCase({}, given));
    const twice = validateCase(once);
    expect(twice).toEqual(once);
    expect(Object.hasOwn(twice.given.identity!.attributes!, "__proto__")).toBe(true);
    expect(Object.hasOwn(twice.given.request!.params!, "__proto__")).toBe(true);
    expect(Object.hasOwn(twice.given.request!.headers!, "__proto__")).toBe(true);
    expect(twice.given.client!.customProperties!["__proto__"]).toBe("c");
    expect(Object.getPrototypeOf(twice.given.identity!.attributes!)).toBe(Object.prototype);
  });

  it("judges value setters with the matcher and flags an undeclared one", () => {
    expect(judge(atmCase({ accessToken: { nonce: "n1" } }), tokenEffects({ nonce: "n1" })).pass).toBe(true);
    expect(judge(atmCase({ accessToken: { nonce: "n1" } }), tokenEffects({ nonce: "n2" })).pass).toBe(false);
    expect(judge(atmCase(), tokenEffects({ authLevel: 2 })).mismatches.map((m) => m.path)).toEqual(["authLevel"]);
  });

  it("fails a refused token unless the case expects the refusal", () => {
    const refusal = 'Cannot invoke "java.util.Collection.size()" because "c" is null';
    const unexpected = judge(atmCase({ allowUndeclared: { accessToken: true } }), tokenEffects({}, refusal));
    expect(unexpected.pass).toBe(false);
    expect(unexpected.summary).toContain(refusal);
    expect(judge(atmCase({ accessToken: { issued: false } }), tokenEffects({}, refusal)).pass).toBe(true);
    expect(judge(atmCase({ accessToken: { issued: false } }), tokenEffects({})).pass).toBe(false);
  });

  it("fails when the runner recorded no token, and ignores the token channel on the decision node", () => {
    expect(judge(atmCase(), makeEffects({ outcome: null })).pass).toBe(false);
    expect(judge(makeCase(), makeEffects()).pass).toBe(true);
  });
});
