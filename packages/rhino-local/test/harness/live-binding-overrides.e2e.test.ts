import { describe, expect, it } from "vitest";
import { aicWhenEnabled, defineSuite, useLease } from "../../src/harness/index.ts";

const suite = defineSuite({
  name: "binding-replacement-channel",
  script: [
    'var selected = logger.getName() + "|" + idRepository.getIdentity("probe");',
    'nodeState.putShared("selected", selected);',
    'action.goTo(String(selected) === "test|hook" ? "matched" : "other");',
  ].join("\n"),
  outcomes: ["matched", "other"],
  always: {
    bindingOverrides: { logger: '({getName: function(){return "suite";}})' },
  },
  beforeRun: ({ request }) => {
    request.bindingOverrides.idRepository = '({getIdentity: function(){return "hook";}})';
  },
});

describe("binding replacements", () => {
  const aic = aicWhenEnabled("live-binding-overrides");
  const lease = useLease(suite, {
    ...aic,
    ...(aic.aic === undefined ? {} : { aic: { ...aic.aic, unsupported: "skip" as const } }),
  });

  it("uses all three channel surfaces and reports the AIC observation gap", async () => {
    const run = await lease.run()
      .bindingOverrides({ logger: '({getName: function(){return "test";}})' })
      .expect({ outcome: "matched", sharedState: { added: { selected: "test|hook" } } });
    expect(run.verdict.pass, run.verdict.summary).toBe(true);
    if (aic.aic !== undefined) {
      expect(run.conformance?.passes[0]?.aic.skipped).toMatch(/bindingOverrides/);
      expect(run.conformance?.observationGaps).toEqual(expect.arrayContaining([
        expect.objectContaining({ path: "aic-lane", aic: "ineligible" }),
      ]));
    }
  });
});
