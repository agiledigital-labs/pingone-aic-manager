import { describe, expect, it } from "vitest";
import { decideFromState, writeState } from "../../cases/index.ts";
import { conform } from "../../src/aic/conform.ts";
import { diffRecordedEffects } from "../../src/aic/diff.ts";
import { assembleEffects } from "../../src/aic/record.ts";
import { judge } from "../../src/case/verdict.ts";
import { caseWith } from "./helpers.ts";
import { makeEffects, bucket } from "../case/helpers.ts";

describe("diffRecordedEffects", () => {
  it("is silent when both lanes recorded the same effects", () => {
    const effects = makeEffects({
      sharedState: bucket({}, { verified: true }),
    });
    expect(diffRecordedEffects(effects, effects)).toEqual({
      disagreements: [],
      observationGaps: [],
    });
  });

  it("names a genuinely different observable outcome", () => {
    const comparison = diffRecordedEffects(
      makeEffects({ outcome: "true" }),
      makeEffects({ outcome: "false" })
    );
    expect(comparison.disagreements).toEqual([
      {
        channel: "outcome",
        path: "outcome",
        local: '"true"',
        aic: '"false"',
        message: 'outcome: local "true", AIC "false"',
      },
    ]);
    expect(comparison.observationGaps).toEqual([]);
  });

  it("reports an unobserved effect channel as a gap, not equality", () => {
    const local = makeEffects({
      openidm: [{ method: "read", resource: "managed/alpha_user/alice" }],
    });
    const aic = makeEffects({
      evidence: {
        ambientState: {},
        unbucketedState: [],
        unobservedChannels: ["openidm"],
      },
    });
    const comparison = diffRecordedEffects(local, aic);
    expect(comparison.disagreements).toEqual([]);
    expect(comparison.observationGaps).toContainEqual(
      expect.objectContaining({ channel: "openidm", aic: "unobserved" })
    );
  });

  it("qualifies equal state mutations when AIC cannot observe the bucket", () => {
    const local = makeEffects({
      transientState: bucket({}, { scratch: "n/a" }),
    });
    const aic = makeEffects({
      evidence: {
        ambientState: {},
        unbucketedState: [
          {
            operation: "added",
            key: "scratch",
            after: "n/a",
            possibleBuckets: ["sharedState", "transientState"],
          },
        ],
        unobservedChannels: [],
      },
    });
    const comparison = diffRecordedEffects(local, aic);
    expect(comparison.disagreements).toEqual([]);
    expect(comparison.observationGaps).toContainEqual(
      expect.objectContaining({ channel: "nodeState", path: "scratch" })
    );
  });

  it("anti-silencing: a wrong unbucketed value is a real disagreement and failed verdict", () => {
    const local = makeEffects({
      transientState: bucket({}, { scratch: "n/a" }),
    });
    const aic = makeEffects({
      evidence: {
        ambientState: {},
        unbucketedState: [
          {
            operation: "added",
            key: "scratch",
            after: "WRONG",
            possibleBuckets: ["sharedState", "transientState"],
          },
        ],
        unobservedChannels: [],
      },
    });
    const verdict = judge(writeState, aic);
    const comparison = diffRecordedEffects(local, aic);
    expect(verdict.pass).toBe(false);
    expect(verdict.mismatches).toContainEqual(
      expect.objectContaining({ channel: "transientState", path: "scratch" })
    );
    expect(comparison.disagreements).toContainEqual(
      expect.objectContaining({ channel: "nodeState", path: "scratch" })
    );
  });

  it("anti-silencing: a different mutation operation is a real disagreement", () => {
    const local = makeEffects({
      transientState: bucket({}, { scratch: "n/a" }),
    });
    const aic = makeEffects({
      evidence: {
        ambientState: {},
        unbucketedState: [
          {
            operation: "changed",
            key: "scratch",
            before: "old",
            after: "n/a",
            possibleBuckets: ["sharedState", "transientState"],
          },
        ],
        unobservedChannels: [],
      },
    });
    const comparison = diffRecordedEffects(local, aic);
    expect(comparison.disagreements).toContainEqual(
      expect.objectContaining({ channel: "nodeState", path: "scratch" })
    );
  });

  it("fails an undeclared unbucketed mutation under default strictness", () => {
    const effects = makeEffects({
      evidence: {
        ambientState: {},
        unbucketedState: [
          {
            operation: "added",
            key: "surprise",
            after: true,
            possibleBuckets: ["sharedState", "transientState"],
          },
        ],
        unobservedChannels: [],
      },
    });
    const verdict = judge(caseWith(), effects);
    expect(verdict.pass).toBe(false);
    expect(verdict.mismatches).toContainEqual(
      expect.objectContaining({ channel: "nodeState", path: "surprise" })
    );
  });

  it("does not let one bucket's allowUndeclared excuse a possibly-disallowed write", () => {
    const kase = caseWith({
      expect: {
        outcome: "true",
        allowUndeclared: { sharedState: true },
      },
    });
    const effects = makeEffects({
      evidence: {
        ambientState: {},
        unbucketedState: [
          {
            operation: "added",
            key: "surprise",
            after: true,
            possibleBuckets: ["sharedState", "transientState"],
          },
        ],
        unobservedChannels: [],
      },
    });
    expect(judge(kase, effects)).toMatchObject({ pass: false });
  });

  it.each([
    {
      label: "changes",
      before: { platform: "before" },
      final: { platform: "after" },
    },
    { label: "removes", before: { platform: "before" }, final: {} },
  ])("fails when the subject $label ambient state", ({ before, final }) => {
    const effects = assembleEffects({
      given: {},
      dump: { outcome: "true", before, final },
      callbacks: [],
    });
    const verdict = judge(caseWith(), effects);
    expect(verdict.pass).toBe(false);
    expect(verdict.mismatches).toContainEqual(
      expect.objectContaining({ channel: "nodeState", path: "platform" })
    );
  });

  it("qualifies expectations on a wholly unobserved channel", () => {
    const kase = caseWith({
      expect: {
        outcome: "true",
        openidm: [{ method: "read", resource: "managed/alpha_user/alice" }],
      },
    });
    const effects = makeEffects({
      evidence: {
        ambientState: {},
        unbucketedState: [],
        unobservedChannels: ["openidm"],
      },
    });
    const verdict = judge(kase, effects);
    expect(verdict).toMatchObject({ pass: true, conclusive: false });
    expect(verdict.unverified).toContainEqual(
      expect.objectContaining({ channel: "openidm" })
    );
  });
});

describe("conform", () => {
  it("judges both lanes and keeps disagreement, gap and ambient reports separate", async () => {
    const localEffects = makeEffects({
      sharedState: bucket({}, { checked: true }),
      transientState: bucket({}, { scratch: "n/a" }),
    });
    const aicEffects = assembleEffects({
      given: writeState.given,
      dump: {
        outcome: "true",
        before: { platform: 42 },
        final: { platform: 42, checked: true, scratch: "n/a" },
      },
      callbacks: [],
    });
    const report = await conform({
      kase: writeState,
      source: writeState.script,
      local: async () => localEffects,
      aic: async () => aicEffects,
    });
    expect(report.local.verdict).toMatchObject({ pass: true, conclusive: true });
    expect(report.aic.verdict).toMatchObject({ pass: true, conclusive: false });
    expect(report.disagreements).toEqual([]);
    expect(report.observationGaps).toEqual(
      expect.arrayContaining([
        expect.objectContaining({ channel: "nodeState", path: "checked" }),
        expect.objectContaining({ channel: "nodeState", path: "scratch" }),
        expect.objectContaining({ channel: "openidm" }),
        expect.objectContaining({ channel: "http" }),
        expect.objectContaining({ channel: "logs" }),
      ])
    );
    expect(report.ambientState).toEqual([
      { lane: "aic", values: { platform: 42 } },
    ]);
  });

  it("decide-from-state passes while reporting pre-subject ambient state separately", async () => {
    const aicEffects = assembleEffects({
      given: decideFromState.given,
      dump: {
        outcome: "true",
        before: { username: "alice", platform: "ambient" },
        final: { username: "alice", platform: "ambient" },
      },
      callbacks: [],
    });
    const report = await conform({
      kase: decideFromState,
      source: decideFromState.script,
      local: async () => makeEffects({
        sharedState: bucket({ username: "alice" }, { username: "alice" }),
      }),
      aic: async () => aicEffects,
    });
    expect(report.aic.verdict).toMatchObject({ pass: true, conclusive: false });
    expect(report.disagreements).toEqual([]);
    expect(report.ambientState).toEqual([
      { lane: "aic", values: { platform: "ambient" } },
    ]);
  });

  it("skips AIC for environment-dependent cases instead of running them", async () => {
    const report = await conform({
      kase: caseWith({
        given: { managed: { alpha_user: [{ userName: "alice" }] } },
      }),
      source: 'action.goTo("true");',
      local: async () => makeEffects(),
    });
    expect(report.portable).toBe(false);
    expect(report.aic.skipped).toMatch(/managed/);
    expect(report.disagreements).toEqual([]);
    expect(report.observationGaps).toEqual([]);
  });

  it("skips missing runners with stated reasons", async () => {
    const report = await conform({
      kase: caseWith(),
      source: 'action.goTo("true");',
    });
    expect(report.local.skipped).toMatch(/bindings lane/);
    expect(report.aic.skipped).toMatch(/no AIC runner/);
    expect(report.disagreements).toEqual([]);
  });
});
