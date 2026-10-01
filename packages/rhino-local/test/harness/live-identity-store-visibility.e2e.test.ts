import { describe, expect, it } from "vitest";
import { aicWhenEnabled, defineSuite, managed, useLease } from "../../src/harness/index.ts";

const ID = "00000000-0000-4000-8000-0000000000d6";
const RESOURCE = `managed/alpha_user/${ID}`;

const suite = defineSuite({
  name: "identity-store-visibility",
  script: [
    'function t(f) { try { return String(f()); } catch (e) { return "threw:" + e; } }',
    `var path = "managed/alpha_user/${ID}";`,
    "if (callbacks.isEmpty()) {",
    `  var a = idRepository.getIdentity("${ID}");`,
    '  a.setAttribute("fr-attr-str1", ["new"]);',
    '  a.setAttribute("fr-attr-multi1", ["only"]);',
    '  a.setAttribute("mail", ["identity-store-after@example.com"]);',
    '  nodeState.putShared("storeResult", t(function () { a.store(); return "ok"; }));',
    '  nodeState.putShared("sameAfter", t(function () { return a.getAttributeValues("fr-attr-str1").toArray()[0]; }));',
    `  nodeState.putShared("freshAfter", t(function () { return idRepository.getIdentity("${ID}").getAttributeValues("fr-attr-str1").toArray()[0]; }));`,
    '  nodeState.putShared("idmMultiSamePass", t(function () { return JSON.stringify(openidm.read(path, null, ["frUnindexedMultivalued1"]).frUnindexedMultivalued1); }));',
    '  nodeState.putShared("idmStringSamePass", t(function () { return JSON.stringify(openidm.read(path, null, ["frUnindexedString1"]).frUnindexedString1); }));',
    '  nodeState.putShared("idmMailSamePass", t(function () { return JSON.stringify(openidm.read(path, null, ["mail"]).mail); }));',
    `  var b = idRepository.getIdentity("${ID}");`,
    '  b.setAttribute("fr-attr-str2", ["nostore"]);',
    '  callbacksBuilder.nameCallback("Continue");',
    "} else {",
    '  nodeState.putShared("idmMulti", t(function () { return JSON.stringify(openidm.read(path, null, ["frUnindexedMultivalued1"]).frUnindexedMultivalued1); }));',
    `  nodeState.putShared("identityMultiSize", t(function () { return idRepository.getIdentity("${ID}").getAttributeValues("fr-attr-multi1").size(); }));`,
    `  nodeState.putShared("noStoreFresh", t(function () { return idRepository.getIdentity("${ID}").getAttributeValues("fr-attr-str2").toArray()[0]; }));`,
    '  nodeState.putShared("noStoreIdm", t(function () { return openidm.read(path, null, ["frUnindexedString2"]).frUnindexedString2; }));',
    '  action.goTo("done");',
    "}",
  ].join("\n"),
  outcomes: ["done"],
  fixtures: {
    subject: managed("managed/alpha_user", {
      _id: ID, userName: "identity-store-probe", mail: "identity-store-probe@example.com",
      givenName: "Identity", sn: "Probe", frUnindexedString1: "old", frUnindexedString2: "keep",
      frUnindexedMultivalued1: ["first", "second"],
    }),
  },
  cleanup: async (idm) => {
    await idm.delete(RESOURCE);
  },
});

describe("identity store visibility", () => {
  const lease = useLease(suite, aicWhenEnabled("live-identity-store-visibility"));

  // Measured 2026-10-01, in one pass: after store() the same wrapper and a
  // fresh getIdentity both read the new value; IDM holds a one-element
  // multivalued write as an array and the single-valued ones as strings. A
  // setAttribute with no store() (wrapper b) never reaches IDM, even across
  // a callback round trip.
  it("shows stored values at once, keeps cardinality, and drops an unstored write", async () => {
    const run = await lease.run().step({
      expect: {
        callbacks: [{ type: "NameCallback", prompt: "Continue" }],
        sharedState: { added: {
          storeResult: "ok",
          sameAfter: "new",
          freshAfter: "new",
          idmMultiSamePass: '["only"]',
          idmStringSamePass: '"new"',
          idmMailSamePass: '"identity-store-after@example.com"',
        } },
        identityWrites: [
          { identity: ID, attribute: "fr-attr-str1", values: ["new"] },
          { identity: ID, attribute: "fr-attr-multi1", values: ["only"] },
          { identity: ID, attribute: "mail", values: ["identity-store-after@example.com"] },
        ],
      },
      reply: [{ type: "NameCallback", value: "continue" }],
    }).expect({
      outcome: "done",
      sharedState: { added: {
        idmMulti: '["only"]',
        identityMultiSize: "1",
        noStoreFresh: "keep",
        noStoreIdm: "keep",
      } },
    });
    expect(run.verdict.pass).toBe(true);
  });
});
