import { describe, expect, it } from "vitest";
import { conform } from "../../src/aic/conform.ts";
import { diffRecordedEffects } from "../../src/aic/diff.ts";
import { assembleEffects } from "../../src/aic/record.ts";
import { caseWith } from "./helpers.ts";
import { makeEffects, bucket } from "../case/helpers.ts";

describe("diffRecordedEffects", () => {
  it("is silent when both lanes recorded the same effects", () => {
    const effects = makeEffects({
      sharedState: bucket({ username: "alice" }, { username: "alice", verified: true }),
    });
    expect(diffRecordedEffects(effects, effects)).toEqual([]);
  });

  it("names the channel where local and AIC disagree", () => {
    const local = makeEffects({ outcome: "true" });
    const aic = makeEffects({ outcome: "false" });
    expect(diffRecordedEffects(local, aic)).toEqual([
      {
        channel: "outcome",
        path: "outcome",
        local: '"true"',
        aic: '"false"',
        message: 'outcome: local "true", AIC "false"',
      },
    ]);
  });

  it("reports an openidm write the AIC lane could not observe", () => {
    const local = makeEffects({
      openidm: [{ method: "read", resource: "managed/alpha_user/alice" }],
    });
    const aic = makeEffects();
    const disagreements = diffRecordedEffects(local, aic);
    expect(disagreements).toHaveLength(1);
    expect(disagreements[0]?.channel).toBe("openidm");
    expect(disagreements[0]?.local).toContain("managed/alpha_user/alice");
    expect(disagreements[0]?.aic).toBe("(none)");
  });
});

describe("conform", () => {
  it("judges both lanes with verdict.ts and diffs their effects", async () => {
    const kase = caseWith({
      given: { sharedState: { username: "alice" } },
      expect: { outcome: "true", sharedState: { added: { verified: true } } },
    });
    const localEffects = assembleEffects({
      given: kase.given,
      dump: {
        outcome: "true",
        before: { username: "alice" },
        final: { username: "alice", verified: true },
      },
      callbacks: [],
    });
    const aicEffects = assembleEffects({
      given: kase.given,
      dump: {
        outcome: "false",
        before: { username: "alice" },
        final: { username: "alice" },
      },
      callbacks: [],
    });
    const report = await conform({
      kase,
      source: "action.goTo('true');",
      local: async () => localEffects,
      aic: async () => aicEffects,
    });
    expect(report.portable).toBe(true);
    expect(report.local.verdict?.pass).toBe(true);
    expect(report.aic.verdict?.pass).toBe(false);
    expect(report.disagreements.map((item) => item.channel).sort()).toEqual([
      "outcome",
      "sharedState",
    ]);
  });

  it("skips AIC for environment-dependent cases instead of running them", async () => {
    const report = await conform({
      kase: caseWith({
        given: { managed: { alpha_user: [{ userName: "alice" }] } },
      }),
      source: "action.goTo('true');",
      local: async () => makeEffects(),
    });
    expect(report.portable).toBe(false);
    expect(report.aic.skipped).toMatch(/managed/);
    expect(report.disagreements).toEqual([]);
  });

  it("skips a missing local runner with a stated reason", async () => {
    const report = await conform({
      kase: caseWith(),
      source: "action.goTo('true');",
    });
    expect(report.local.skipped).toMatch(/bindings lane/);
    expect(report.aic.skipped).toMatch(/no AIC runner/);
  });
});
