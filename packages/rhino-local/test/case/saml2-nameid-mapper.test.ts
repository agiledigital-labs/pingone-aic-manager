/**
 * The SAML2_NAMEID_MAPPER context's case validation and verdict, without a JVM.
 * The e2e that runs the overlay is test/bindings/saml2-nameid-mapper.e2e.test.ts.
 */
import { describe, expect, it } from "vitest";
import { aicUnsupportedReason } from "../../src/aic/unsupported.ts";
import { defineCase, judge, validateCase } from "../../src/case/index.ts";
import type { JsonObject, NameIdEffect } from "../../src/case/index.ts";
import { makeEffects } from "./helpers.ts";

const CTX = "SAML2_NAMEID_MAPPER" as const;
const EMAIL = "urn:oasis:names:tc:SAML:1.1:nameid-format:emailAddress";
const niCase = (expect: JsonObject = {}, given: JsonObject = {}) =>
  ({ name: "ni", script: "x.js", context: CTX, given, expect }) as unknown;

function effects(overrides: Partial<NameIdEffect> = {}) {
  const nameId: NameIdEffect = {
    accepted: true,
    value: "zzemuR3S-nameid-return",
    format: EMAIL,
    refusal: null,
    ...overrides,
  };
  return { ...makeEffects({ outcome: null }), nameId };
}

const refused = (refusal: string): Partial<NameIdEffect> => ({
  accepted: false, value: null, refusal,
});

describe("SAML2_NAMEID_MAPPER case validation", () => {
  it("refuses a NameID key on the decision node and names the selector", () => {
    for (const body of [
      { expect: { outcome: "true", nameId: { value: "a" } } },
      { expect: { outcome: "true", allowUndeclared: { nameId: true } } },
      { given: { nameId: {} }, expect: { outcome: "true" } },
    ]) {
      expect(() => validateCase({ name: "n", script: "s", ...body })).toThrow(/set case\.context/);
    }
  });

  it("refuses node state and other contexts' keys", () => {
    expect(() => validateCase(niCase({}, { sharedState: {} }))).toThrow(/given\.sharedState is not accepted in context/);
    expect(() => validateCase(niCase({}, { accessToken: {} }))).toThrow(/is not accepted in context/);
    expect(() => validateCase(niCase({ callbacks: [] }))).toThrow(/expect\.callbacks is not accepted in context/);
    expect(() => validateCase(niCase({ mayAct: {} }))).toThrow(/expect\.mayAct is not accepted in context/);
  });

  it("accepts the measured identity and the NameID scalars", () => {
    const kase = defineCase({
      name: "ok",
      script: "x.js",
      context: CTX,
      given: {
        identity: { name: "subject-id", exists: true, attributes: { mail: ["zzemuR3S@example.com"] } },
        nameId: {
          format: EMAIL,
          remoteEntityId: "http://localhost:18080/realms/zzemuR3S",
          hostedEntityId: "https://zzemuR3S-idp.example.com",
          mappedValue: "zzemuR3S@example.com",
        },
      },
      expect: { nameId: { value: "zzemuR3S-nameid-return", format: EMAIL } },
    });
    expect(kase.expect.nameId).toEqual({ value: "zzemuR3S-nameid-return", format: EMAIL });
    expect(kase.given.identity?.attributes).toEqual({ mail: ["zzemuR3S@example.com"] });
  });

  it("refuses an identity fixture this context did not measure", () => {
    expect(() => validateCase(niCase({}, { identity: { universalId: "id=x" } }))).toThrow(/universalId was not measured/);
    expect(() => validateCase(niCase({}, { identity: { attributes: { uid: ["x"] } } }))).toThrow(/only accepts/);
    expect(() => validateCase(niCase({}, { identity: { attributes: { mail: ["a", "b"] } } }))).toThrow(/only accepts/);
    expect(() => validateCase(niCase({}, { nameId: { extra: true } }))).toThrow(/extra/);
  });

  it("requires a NameID value unless the case expects the measured refusal", () => {
    expect(() => validateCase(niCase({ nameId: {} }))).toThrow(/value is required/);
    expect(() => validateCase(niCase({ nameId: { accepted: true } }))).toThrow(/value is required/);
    expect(() => validateCase(niCase({ nameId: { accepted: false, value: "x" } }))).toThrow(/cannot declare value/);
    expect(() => validateCase(niCase({ nameId: { accepted: "no" } }))).toThrow(/accepted must be a boolean/);
    const refusedCase = defineCase({
      name: "void",
      script: "x.js",
      context: CTX,
      expect: { nameId: { accepted: false, refusal: "Could not generate NameID" } },
    });
    expect(refusedCase.expect.nameId).toEqual({ accepted: false, refusal: "Could not generate NameID" });
  });

  it("runs locally only", () => {
    expect(aicUnsupportedReason(validateCase(niCase({ nameId: { value: "a" } })) as never)).toMatch(/another script context runs locally only/);
  });
});

describe("SAML2_NAMEID_MAPPER verdict", () => {
  it("fails a case whose run recorded no result", () => {
    const verdict = judge(niCase({ nameId: { value: "zzemuR3S-nameid-return" } }), makeEffects({ outcome: null }));
    expect(verdict.mismatches.map((m) => m.path)).toEqual(["nameId"]);
  });

  it("compares the NameID text and, only when declared, the format", () => {
    expect(judge(niCase({ nameId: { value: "zzemuR3S-nameid-return" } }), effects()).pass).toBe(true);
    expect(judge(niCase({ nameId: { value: "other" } }), effects()).mismatches.map((m) => m.path)).toEqual(["value"]);
    expect(judge(niCase({ nameId: { value: "zzemuR3S-nameid-return", format: EMAIL } }), effects()).pass).toBe(true);
    const wrongFormat = judge(niCase({ nameId: { value: "zzemuR3S-nameid-return", format: "other" } }), effects());
    expect(wrongFormat.mismatches.map((m) => m.path)).toEqual(["format"]);
  });

  it("calls an undeclared NameID a mismatch unless allowUndeclared.nameId", () => {
    expect(judge(niCase(), effects()).mismatches.map((m) => m.path)).toEqual(["value"]);
    expect(judge(niCase({ allowUndeclared: { nameId: true } }), effects()).pass).toBe(true);
  });

  it("expects a NameID unless the case declares the measured refusal", () => {
    const failed = judge(niCase({ nameId: { value: "zzemuR3S-nameid-return" } }), effects(refused("Could not generate NameID")));
    expect(failed.mismatches.map((m) => m.path)).toEqual(["accepted"]);
    expect(failed.mismatches[0]!.message).toMatch(/Could not generate NameID/);
    const undeclared = judge(niCase({ nameId: { accepted: false } }), effects(refused("Could not generate NameID")));
    expect(undeclared.mismatches.map((m) => m.path)).toEqual(["refusal"]);
    expect(judge(niCase({ nameId: { accepted: false, refusal: "Could not generate NameID" } }), effects(refused("Could not generate NameID"))).pass).toBe(true);
    const wrong = judge(niCase({ nameId: { accepted: false, refusal: "other" } }), effects(refused("Could not generate NameID")));
    expect(wrong.mismatches.map((m) => m.path)).toEqual(["refusal"]);
    expect(judge(niCase({ nameId: { accepted: false }, allowUndeclared: { nameId: true } }), effects(refused("Could not generate NameID"))).pass).toBe(true);
  });
});
