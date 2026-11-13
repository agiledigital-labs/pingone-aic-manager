/**
 * The OAUTH2_MAY_ACT context's case validation and verdict, without a JVM.
 * The e2e conformance against live captures is
 * test/bindings/oauth2-may-act.e2e.test.ts.
 */
import { describe, expect, it } from "vitest";
import { aicUnsupportedReason } from "../../src/aic/unsupported.ts";
import { defineCase, judge, validateCase } from "../../src/case/index.ts";
import type { JsonObject, MayActEffect } from "../../src/case/index.ts";
import { makeEffects } from "./helpers.ts";

const MA = "OAUTH2_MAY_ACT" as const;
const maCase = (expect: JsonObject = {}, given: JsonObject = {}) =>
  ({ name: "ma", script: "x.js", context: MA, given, expect }) as unknown;

function effects(overrides: Partial<MayActEffect> = {}) {
  const mayAct: MayActEffect = {
    status: 200,
    mayActPresent: true,
    mayAct: { client_id: "actor" },
    actPresent: false,
    act: null,
    scope: ["read"],
    seedScope: ["read"],
    refusal: null,
    ...overrides,
  };
  return { ...makeEffects({ outcome: null }), mayAct };
}
const failed = (refusal: string): Partial<MayActEffect> => ({
  status: 500, mayActPresent: false, mayAct: null, actPresent: false, act: null, scope: null, refusal,
});

describe("OAUTH2_MAY_ACT case validation", () => {
  it("refuses a may-act key on the decision node and names the selector", () => {
    for (const [section, body] of [
      ["expect", { expect: { outcome: "true", mayAct: {} } }],
      ["allowUndeclared", { expect: { outcome: "true", allowUndeclared: { mayAct: true } } }],
    ] as const) {
      expect(() => validateCase({ name: "n", script: "s", ...body }), section).toThrow(/set case\.context/);
    }
  });

  it("refuses node state, callbacks and other contexts' keys", () => {
    expect(() => validateCase(maCase({}, { sharedState: {} }))).toThrow(/given\.sharedState is not accepted in context/);
    expect(() => validateCase(maCase({}, { validateScope: {} }))).toThrow(/is not accepted in context/);
    expect(() => validateCase(maCase({ callbacks: [] }))).toThrow(/expect\.callbacks is not accepted in context/);
    expect(() => validateCase(maCase({ validateScope: {} }))).toThrow(/expect\.validateScope is not accepted in context/);
  });

  it("accepts the token seeds, and keeps JSON null distinct from an omitted claim", () => {
    {
      const engine = "next-gen" as const;
      const kase = defineCase({
        name: "ok",
        script: "x.js",
        context: MA,
        given: {
          engine,
          client: { clientId: "client-a" },
          scopes: ["read"],
          request: { params: { grant_type: ["client_credentials"] } },
          accessToken: { grantType: "client_credentials" },
          identity: { name: "client-a" },
        },
        expect: { mayAct: { mayAct: null, scope: ["read"] } },
      });
      expect(kase.expect.mayAct).toEqual({ mayAct: null, scope: ["read"] });
      expect("mayAct" in (kase.expect.mayAct ?? {})).toBe(true);
    }
  });

  it("P2-5 refuses access-token members and grants this context does not implement", () => {
    expect(() => validateCase(maCase({}, { accessToken: { fields: { custom: "seeded" } } }))).toThrow(/fields\.custom/);
    expect(() => validateCase(maCase({}, { accessToken: { fields: { expires_in: 60, custom: "seeded" } } }))).toThrow(/custom/);
    expect(() => validateCase(maCase({}, { accessToken: { resourceOwnerId: "user" } }))).toThrow(/resourceOwnerId/);
    expect(() => validateCase(maCase({}, { accessToken: { audience: ["a"] } }))).toThrow(/audience/);
    expect(() => validateCase(maCase({}, { accessToken: { nonce: "n" } }))).toThrow(/nonce/);
    expect(() => validateCase(maCase({}, { accessToken: { claims: "c" } }))).toThrow(/claims/);
    expect(() => validateCase(maCase({}, { accessToken: { grantType: "authorization_code" } }))).toThrow(/client_credentials/);
    expect(() => validateCase(maCase({}, { request: { params: { grant_type: ["authorization_code"] } } }))).toThrow(/grant_type/);
  });

  it("P2-5 accepts the measured access-token subset", () => {
    const kase = defineCase({
      name: "subset",
      script: "x.js",
      context: MA,
      given: {
        accessToken: { clientId: "client-a", grantType: "client_credentials", realm: "/alpha", scope: ["read"], fields: { expires_in: 60 } },
      },
      expect: { mayAct: { issued: false } },
    });
    expect(kase.given.accessToken).toEqual({
      clientId: "client-a", grantType: "client_credentials", realm: "/alpha", scope: ["read"], fields: { expires_in: 60 },
    });
  });

  it("checks the expectation by shape", () => {
    expect(() => validateCase(maCase({ mayAct: { issued: "no" } }))).toThrow(/issued must be a boolean/);
    expect(() => validateCase(maCase({ mayAct: { scope: ["a", "a"] } }))).toThrow(/lists "a" twice/);
    expect(() => validateCase(maCase({ mayAct: { mayAct: { client_id: "a" }, issued: false } }))).toThrow(/cannot declare mayAct, act or scope/);
    expect(() => validateCase(maCase({ mayAct: { extra: true } }))).toThrow(/extra/);
  });

  it("refuses a repeated seed scope, which would let the unchanged-scope check pass a widened grant", () => {
    // Seed ["a", "a"] against a granted ["a", "b"] has equal lengths and every
    // seed entry granted, so a list comparison reads it as unchanged.
    expect(() => validateCase(maCase({}, { scopes: ["a", "a"] }))).toThrow(/given\.scopes lists "a" twice; a scope is a set/);
    expect(validateCase(maCase({}, { scopes: ["a", "b"] }))).toMatchObject({ given: { scopes: ["a", "b"] } });
  });

  it("runs locally only", () => {
    expect(aicUnsupportedReason(validateCase(maCase()) as never)).toMatch(/another script context runs locally only/);
  });
});

describe("OAUTH2_MAY_ACT verdict", () => {
  it("fails a case whose run recorded no result", () => {
    const verdict = judge(maCase(), makeEffects({ outcome: null }));
    expect(verdict.mismatches.map((m) => m.path)).toEqual(["mayAct"]);
  });

  it("treats a JSON-null claim as present and an omitted claim as absent", () => {
    expect(judge(maCase({ mayAct: { mayAct: null, act: null } }), effects({
      mayActPresent: true, mayAct: null, actPresent: true, act: null,
    })).pass).toBe(true);
    const absent = judge(maCase({ mayAct: { mayAct: null } }), effects({ mayActPresent: false, mayAct: null }));
    expect(absent.mismatches.map((m) => m.path)).toEqual(["mayAct"]);
    expect(absent.mismatches[0]!.actual).toBe("<absent>");
  });

  it("calls an undeclared claim a mismatch unless allowUndeclared.mayAct", () => {
    expect(judge(maCase(), effects()).mismatches.map((m) => m.path)).toEqual(["mayAct"]);
    expect(judge(maCase({ allowUndeclared: { mayAct: true } }), effects()).pass).toBe(true);
  });

  it("compares scope as a set, and flags a grant that differs from the request", () => {
    expect(judge(maCase({ mayAct: { mayAct: { client_id: "actor" }, scope: ["read"] } }), effects()).pass).toBe(true);
    const widened = effects({ scope: ["read", "write"] });
    expect(judge(maCase({ mayAct: { mayAct: { client_id: "actor" } } }), widened).mismatches.map((m) => m.path)).toEqual(["scope"]);
    expect(judge(maCase({ mayAct: { mayAct: { client_id: "actor" }, scope: ["write", "read"] } }), widened).pass).toBe(true);
    expect(judge(maCase({ allowUndeclared: { mayAct: true } }), widened).pass).toBe(true);
  });

  it("expects a token unless the case declares a refusal", () => {
    const refused = judge(maCase(), effects(failed("Error running may_act script")));
    expect(refused.mismatches.map((m) => m.path)).toEqual(["issued"]);
    expect(refused.mismatches[0]!.message).toMatch(/500: Error running may_act script/);
    expect(judge(maCase({ mayAct: { issued: false } }), effects(failed("Error running may_act script"))).pass).toBe(true);
    const wrong = judge(maCase({ mayAct: { issued: false, refusal: "other" } }), effects(failed("Error running may_act script")));
    expect(wrong.mismatches.map((m) => m.path)).toEqual(["refusal"]);
    expect(judge(maCase({ mayAct: { issued: false } }), effects()).mismatches.map((m) => m.path)).toEqual(["issued"]);
  });
});
