import { describe, expect, it } from "vitest";
import { decideFromState, writeState } from "../../cases/index.ts";
import {
  chainFromRunResult,
  conform,
  conformChain,
} from "../../src/aic/conform.ts";
import { diffRecordedEffects } from "../../src/aic/diff.ts";
import { assembleEffects } from "../../src/aic/record.ts";
import { runAicChain, type AicReply } from "../../src/aic/run.ts";
import { judge } from "../../src/case/verdict.ts";
import type { RunResult } from "../../src/harness/lease.ts";
import { caseWith } from "./helpers.ts";
import {
  callbackResponse,
  finalResponse,
  mockChain,
} from "./mock-chain.ts";
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
        stateBuckets: "exact",
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
        stateBuckets: "unified",
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

  it("reports the standing unified-bucket limitation even with no visible delta", () => {
    const aic = makeEffects({
      evidence: {
        stateBuckets: "unified",
        ambientState: {},
        unbucketedState: [],
        unobservedChannels: [],
      },
    });
    expect(diffRecordedEffects(makeEffects(), aic).observationGaps).toContainEqual(
      expect.objectContaining({ channel: "nodeState", path: "buckets" })
    );
  });

  it("classifies a same-value lower-bucket write as structurally hidden", () => {
    const local = makeEffects({
      sharedState: bucket({ existing: 1 }, { existing: 1 }),
      transientState: bucket({}, { existing: 1 }),
    });
    const aic = makeEffects({
      sharedState: bucket({ existing: 1 }, { existing: 1 }),
      evidence: {
        stateBuckets: "unified",
        ambientState: {},
        unbucketedState: [],
        unobservedChannels: [],
      },
    });
    const comparison = diffRecordedEffects(local, aic);
    expect(comparison.disagreements).toEqual([]);
    expect(comparison.observationGaps).toContainEqual(
      expect.objectContaining({ channel: "nodeState", path: "existing" })
    );
  });

  it("anti-silencing: a missing new key is a disagreement despite unified buckets", () => {
    const local = makeEffects({
      transientState: bucket({}, { created: true }),
    });
    const aic = makeEffects({
      evidence: {
        stateBuckets: "unified",
        ambientState: {},
        unbucketedState: [],
        unobservedChannels: [],
      },
    });
    expect(diffRecordedEffects(local, aic).disagreements).toContainEqual(
      expect.objectContaining({ channel: "nodeState", path: "created" })
    );
  });

  it("anti-silencing: a wrong unbucketed value is a real disagreement and failed verdict", () => {
    const local = makeEffects({
      transientState: bucket({}, { scratch: "n/a" }),
    });
    const aic = makeEffects({
      evidence: {
        stateBuckets: "unified",
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
        stateBuckets: "unified",
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
        stateBuckets: "unified",
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
        stateBuckets: "unified",
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
        stateBuckets: "exact",
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

describe("conformChain", () => {
  it("reports every pass and marks intermediate AIC observations as gaps", async () => {
    const cases = [
      caseWith({
        name: "three-pass [step 1]",
        expect: { outcome: null, callbacks: [{ type: "NameCallback" }] },
      }),
      caseWith({
        name: "three-pass [step 2]",
        expect: { outcome: null, callbacks: [{ type: "ChoiceCallback" }] },
      }),
      caseWith({ name: "three-pass", expect: { outcome: "done" } }),
    ];
    const localEffects = [
      makeEffects({ outcome: null, callbacks: [{ type: "NameCallback" }] }),
      makeEffects({ outcome: null, callbacks: [{ type: "ChoiceCallback" }] }),
      makeEffects({ outcome: "done" }),
    ];
    const replies: AicReply[][] = [
      [{ type: "NameCallback", value: "alice" }],
      [{ type: "ChoiceCallback", value: 0 }],
    ];
    const fake = mockChain({
      responses: [
        callbackResponse("NameCallback", "jwt-1"),
        callbackResponse("ChoiceCallback", "jwt-2"),
        finalResponse("done"),
      ],
    });
    const report = await conformChain({
      cases,
      localEffects,
      replies,
      source: 'action.goTo("done");',
      aic: ({ cases: chainCases, source, replies: chainReplies }) =>
        runAicChain(chainCases, source, {
          io: fake.io,
          runId: "conform1",
          project: "/tmp/rhino-local-aic-test",
          replies: chainReplies,
        }),
    });

    expect(report.passes).toHaveLength(3);
    expect(report.passes.map((pass) => pass.aicObserved)).toEqual([
      false,
      false,
      true,
    ]);
    expect(
      report.passes.slice(0, 2).map((pass) => pass.observationGaps)
    ).toEqual([
      [expect.objectContaining({ path: "pass", aic: "unobserved" })],
      [expect.objectContaining({ path: "pass", aic: "unobserved" })],
    ]);
    expect(report.passes[2]?.disagreements).toEqual([]);
    expect(report.passes.every((pass) => pass.local.verdict?.pass === true)).toBe(
      true
    );
    expect(report.passes.every((pass) => pass.aic.verdict?.pass === true)).toBe(
      true
    );
  });

  it("refuses a reply arity mismatch before invoking the AIC lane", async () => {
    let invoked = false;
    await expect(
      conformChain({
        cases: [caseWith({ name: "ask" }), caseWith({ name: "finish" })],
        localEffects: [makeEffects(), makeEffects()],
        replies: [],
        source: 'action.goTo("true");',
        aic: async () => {
          invoked = true;
          return [];
        },
      })
    ).rejects.toThrow(/2 passes need 1 reply sets, got 0/);
    expect(invoked).toBe(false);
  });

  it("skips the whole chain when any pass is unsupported", async () => {
    let invoked = false;
    const report = await conformChain({
      cases: [
        caseWith({ name: "portable" }),
        caseWith({
          name: "tenant-dependent",
          given: { managed: { alpha_user: [{ userName: "alice" }] } },
        }),
      ],
      localEffects: [makeEffects(), makeEffects()],
      replies: [[]],
      source: 'action.goTo("true");',
      aic: async () => {
        invoked = true;
        return [];
      },
    });

    expect(invoked).toBe(false);
    expect(report.passes.map((pass) => pass.aic.skipped)).toEqual([
      expect.stringMatching(/whole chain.*given\.managed/),
      expect.stringMatching(/whole chain.*given\.managed/),
    ]);
    expect(report.passes.every((pass) => pass.aic.effects === undefined)).toBe(
      true
    );
  });

  it("anti-silencing: surfaces a final-effects disagreement", async () => {
    const ask = caseWith({
      name: "final disagreement [step 1]",
      expect: { outcome: null, callbacks: [{ type: "NameCallback" }] },
    });
    const finish = caseWith({
      name: "final disagreement",
      expect: { outcome: "done" },
    });
    const asking = makeEffects({
      outcome: null,
      callbacks: [{ type: "NameCallback" }],
    });
    const fake = mockChain({
      responses: [
        callbackResponse("NameCallback", "jwt-1"),
        finalResponse("denied"),
      ],
    });
    const report = await conformChain({
      cases: [ask, finish],
      localEffects: [asking, makeEffects({ outcome: "done" })],
      replies: [[{ type: "NameCallback", value: "alice" }]],
      source: 'action.goTo("done");',
      aic: ({ cases, source, replies }) =>
        runAicChain(cases, source, {
          io: fake.io,
          runId: "conform2",
          project: "/tmp/rhino-local-aic-test",
          replies,
        }),
    });

    expect(report.passes[1]?.aic.verdict).toMatchObject({ pass: false });
    expect(report.passes[1]?.disagreements).toContainEqual(
      expect.objectContaining({
        channel: "outcome",
        local: '"done"',
        aic: '"denied"',
      })
    );
    expect(report.disagreements).toEqual(report.passes[1]?.disagreements);
  });

  it("bridges a RunResult without submitting unanswered callbacks", () => {
    const stepCase = caseWith({
      name: "bridge [step 1]",
      expect: { outcome: null },
    });
    const finalCase = caseWith({ name: "bridge" });
    const stepEffects = makeEffects({ outcome: null });
    const finalEffects = makeEffects();
    const result: RunResult = {
      kase: finalCase,
      effects: finalEffects,
      verdict: judge(finalCase, finalEffects),
      steps: [
        {
          kase: stepCase,
          effects: stepEffects,
          verdict: judge(stepCase, stepEffects),
          submitted: [
            { type: "NameCallback", value: "alice", prompt: "Name" },
            { type: "HiddenValueCallback", id: "unanswered" },
          ],
        },
      ],
    };

    expect(chainFromRunResult(result)).toEqual({
      cases: [stepCase, finalCase],
      localEffects: [stepEffects, finalEffects],
      replies: [[{ type: "NameCallback", value: "alice" }]],
    });
  });
});
