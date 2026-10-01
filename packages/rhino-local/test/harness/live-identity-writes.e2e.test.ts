import { describe, expect, it } from "vitest";
import { aicWhenEnabled, defineSuite, managed, useLease } from "../../src/harness/index.ts";

const ID = "00000000-0000-4000-8000-0000000000d5";
const RESOURCE = `managed/alpha_user/${ID}`;

const suite = defineSuite({
  name: "identity-write-sequence",
  script: [
    'function t(f) { try { return String(f()); } catch (e) { return "threw:" + e; } }',
    'var identity = null;',
    `var lookupResult = t(function () { identity = idRepository.getIdentity("${ID}"); return identity ? "object" : "null"; });`,
    "if (callbacks.isEmpty()) {",
    '  nodeState.putShared("lookupResult", lookupResult);',
    '  nodeState.putShared("setResult", t(function () { identity.setAttribute("fr-attr-str1", ["new"]); return "ok"; }));',
    '  nodeState.putShared("preStore", t(function () { return identity.getAttributeValues("fr-attr-str1").toArray()[0]; }));',
    '  nodeState.putShared("addResult", t(function () { identity.addAttribute("fr-attr-multi1", "second"); return "ok"; }));',
    '  nodeState.putShared("storeResult", t(function () { identity.store(); return "ok"; }));',
    '  callbacksBuilder.nameCallback("Continue");',
    "} else {",
    '  nodeState.putShared("observedLookup", lookupResult);',
    '  var before = t(function () { return nodeState.get("preStore"); });',
    '  var setResult = t(function () { return nodeState.get("setResult"); });',
    '  var addResult = t(function () { return nodeState.get("addResult"); });',
    '  var storeResult = t(function () { return nodeState.get("storeResult"); });',
    '  var correctName = t(function () { return identity.getAttributeValues("fr-attr-str1").toArray()[0]; });',
    '  var count = t(function () { return identity.getAttributeValues("fr-attr-multi1").size(); });',
    '  var first = t(function () { return identity.getAttributeValues("fr-attr-multi1").contains("first"); });',
    '  var second = t(function () { return identity.getAttributeValues("fr-attr-multi1").contains("second"); });',
    '  var wrongName = t(function () { return identity.getAttributeValues("frUnindexedString1").size(); });',
    '  nodeState.putShared("observedPreStore", before);',
    '  nodeState.putShared("observedSet", setResult);',
    '  nodeState.putShared("observedAdd", addResult);',
    '  nodeState.putShared("observedStore", storeResult);',
    '  nodeState.putShared("observedString", correctName);',
    '  nodeState.putShared("observedCount", count);',
    '  nodeState.putShared("observedFirst", first);',
    '  nodeState.putShared("observedSecond", second);',
    '  nodeState.putShared("observedWrongName", wrongName);',
    '  action.goTo(lookupResult === "object" && before === "old" && setResult === "ok" && addResult === "ok" && storeResult === "ok" && correctName === "new" && count === "2" && first === "true" && second === "true" && wrongName === "0" ? "match" : "mismatch");',
    "}",
  ].join("\n"),
  outcomes: ["match", "mismatch"],
  fixtures: {
    subject: managed("managed/alpha_user", {
      _id: ID, userName: "identity-write-probe", mail: "identity-write-probe@example.com",
      givenName: "Identity", sn: "Probe", frUnindexedString1: "old",
      frUnindexedMultivalued1: ["first"],
    }),
  },
  cleanup: async (idm) => {
    await idm.delete(RESOURCE);
  },
});

describe("identity writes", () => {
  const lease = useLease(suite, aicWhenEnabled("live-identity-writes"));

  it("reads persisted values before store and new values after a callback pass", async () => {
    const run = await lease.run().step({
      expect: {
        callbacks: [{ type: "NameCallback", prompt: "Continue" }],
        sharedState: { added: {
          lookupResult: /^.*$/,
          setResult: /^.*$/,
          preStore: /^.*$/,
          addResult: /^.*$/,
          storeResult: /^.*$/,
        } },
        identityWrites: [
          { identity: ID, attribute: "fr-attr-str1", values: ["new"] },
          { identity: ID, attribute: "fr-attr-multi1", values: ["first", "second"] },
        ],
      },
      reply: [{ type: "NameCallback", value: "continue" }],
    }).expect({
      outcome: "match",
      sharedState: { added: {
        observedLookup: "object",
        observedPreStore: "old",
        observedSet: "ok",
        observedAdd: "ok",
        observedStore: "ok",
        observedString: "new",
        observedCount: "2",
        observedFirst: "true",
        observedSecond: "true",
        observedWrongName: "0",
      } },
    });
    expect(run.verdict.pass).toBe(true);
  });
});
