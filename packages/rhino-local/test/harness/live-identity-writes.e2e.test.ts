import { describe, expect, it } from "vitest";
import { aicWhenEnabled, defineSuite, managed, useLease } from "../../src/harness/index.ts";

const ID = "00000000-0000-4000-8000-0000000000d5";
const RESOURCE = `managed/alpha_user/${ID}`;

const suite = defineSuite({
  name: "identity-write-sequence",
  script: [
    `var identity = idRepository.getIdentity("${ID}");`,
    "if (callbacks.isEmpty()) {",
    '  identity.setAttribute("fr-attr-str1", ["new"]);',
    '  nodeState.putShared("preStore", String(identity.getAttributeValues("fr-attr-str1").toArray()[0]));',
    '  identity.addAttribute("fr-attr-multi1", "second");',
    "  identity.store();",
    '  callbacksBuilder.nameCallback("Continue");',
    "} else {",
    '  var values = identity.getAttributeValues("fr-attr-multi1");',
    '  var correctName = identity.getAttributeValues("fr-attr-str1").toArray()[0];',
    '  var wrongName = identity.getAttributeValues("frUnindexedString1");',
    '  action.goTo(nodeState.get("preStore") === "new" && correctName === "new" && values.size() === 2 && values.contains("first") && values.contains("second") && wrongName.size() === 0 ? "match" : "mismatch");',
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

  it("reads staged values and persists set plus add across a callback pass", async () => {
    const run = await lease.run().step({
      expect: {
        callbacks: [{ type: "NameCallback", prompt: "Continue" }],
        sharedState: { added: { preStore: "new" } },
        openidm: [{ method: "patch", resource: RESOURCE, body: [
          { operation: "replace", field: "frUnindexedString1", value: ["new"] },
          { operation: "replace", field: "frUnindexedMultivalued1", value: ["first", "second"] },
        ] }],
      },
      check: async (idm) => {
        const record = await idm.read(RESOURCE);
        expect(record?.frUnindexedString1).toBe("new");
        expect(record?.frUnindexedMultivalued1).toEqual(["first", "second"]);
      },
      reply: [{ type: "NameCallback", value: "continue" }],
    }).expect({ outcome: "match" });
    expect(run.verdict.pass).toBe(true);
  });
});
