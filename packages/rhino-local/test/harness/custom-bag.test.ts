import { describe, expect, it } from "vitest";
import { defineSuite, useLease } from "../../src/harness/index.ts";

const suite = defineSuite({
  name: "custom-bag-roundtrip",
  script: [
    'var identity = idRepository.getIdentity("example");',
    'if (callbacks.isEmpty()) {',
    '  identity.setAttribute("fr-idm-custom-attrs", nodeState.get("values"));',
    '  identity.store();',
    '  callbacksBuilder.nameCallback("Continue");',
    '} else {',
    '  var v = identity.getAttributeValues("fr-idm-custom-attrs");',
    '  action.goTo(v.size() === 0 ? "absent" : "present");',
    '}',
  ].join("\n"),
  outcomes: ["absent", "present"],
  beforeRun: ({ fixtures }) => fixtures.create("managed/alpha_user", { _id: "example", custom_example: "seeded", custom_flag: true }),
  cleanup: async (idm) => { await idm.delete("managed/alpha_user/example"); },
});

describe("custom bag across real Rhino callback passes", () => {
  const lease = useLease(suite);
  it.each([
    { values: [], outcome: "absent", record: { _id: "example" } },
    { values: ["{}"], outcome: "present", record: { _id: "example" } },
    { values: ['{"custom_flag":false}'], outcome: "present", record: { _id: "example", custom_flag: false } },
  ])("retains $values and the replaced IDM record", async ({ values, outcome, record }) => {
    const run = await lease.run().state({ shared: { values } }).step({
      expect: {
        callbacks: [{ type: "NameCallback", prompt: "Continue" }],
        identityWrites: [{ identity: "example", attribute: "fr-idm-custom-attrs", values }],
      },
      reply: [{ type: "NameCallback", value: "continue" }],
      check: async (idm) => { expect(await idm.read("managed/alpha_user/example")).toEqual(record); },
    }).expect({ outcome });
    expect(run.verdict.pass, run.verdict.summary).toBe(true);
  });
});
