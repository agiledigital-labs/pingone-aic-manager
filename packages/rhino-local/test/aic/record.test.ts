import { describe, expect, it } from "vitest";
import {
  assembleEffects,
  classifyFinal,
  parseSubjectDump,
} from "../../src/aic/record.ts";

describe("classifyFinal", () => {
  it("derives arbitrary ambient state from the before snapshot without a name allow-list", () => {
    const recorded = classifyFinal(
      {
        sharedState: { username: "alice" },
        transientState: { password: "secret" },
      },
      {
        username: "alice",
        password: "secret",
        platformValueNeverNamedByTheHarness: 42,
      },
      {
        username: "alice",
        password: "secret",
        platformValueNeverNamedByTheHarness: 42,
        verified: true,
      }
    );
    expect(recorded.sharedState).toEqual({
      initial: { username: "alice" },
      final: { username: "alice" },
    });
    expect(recorded.transientState).toEqual({
      initial: { password: "secret" },
      final: { password: "secret" },
    });
    expect(recorded.evidence.ambientState).toEqual({
      platformValueNeverNamedByTheHarness: 42,
    });
    expect(recorded.evidence.unbucketedState).toEqual([
      {
        operation: "added",
        key: "verified",
        after: true,
        possibleBuckets: ["sharedState", "transientState"],
      },
    ]);
  });

  it("does not report unchanged ambient state as a script mutation", () => {
    const recorded = classifyFinal({}, { platform: "before" }, { platform: "before" });
    expect(recorded.evidence.ambientState).toEqual({ platform: "before" });
    expect(recorded.evidence.unbucketedState).toEqual([]);
    expect(recorded.sharedState).toEqual({ initial: {}, final: {} });
  });

  it("keeps a changed ambient value as an unbucketed behavioural mutation", () => {
    const recorded = classifyFinal({}, { platform: "before" }, { platform: "after" });
    expect(recorded.evidence.ambientState).toEqual({ platform: "before" });
    expect(recorded.evidence.unbucketedState).toEqual([
      {
        operation: "changed",
        key: "platform",
        before: "before",
        after: "after",
        possibleBuckets: ["sharedState", "transientState", "secureState"],
      },
    ]);
  });

  it("keeps removal of ambient state as an unbucketed behavioural mutation", () => {
    const recorded = classifyFinal({}, { platform: "before" }, {});
    expect(recorded.evidence.unbucketedState).toEqual([
      {
        operation: "removed",
        key: "platform",
        before: "before",
        possibleBuckets: ["sharedState", "transientState", "secureState"],
      },
    ]);
  });

  it("records removal exactly when a declared seed occupied one bucket", () => {
    const recorded = classifyFinal(
      { sharedState: { leftover: 1, keep: 2 } },
      { leftover: 1, keep: 2 },
      { keep: 2 }
    );
    expect(recorded.sharedState).toEqual({
      initial: { leftover: 1, keep: 2 },
      final: { keep: 2 },
    });
    expect(recorded.evidence.unbucketedState).toEqual([]);
  });

  it("refuses to classify a run whose declared seed was not visible", () => {
    expect(() =>
      classifyFinal(
        { sharedState: { username: "alice" } },
        { username: "bob" },
        { username: "bob" }
      )
    ).toThrow(/did not contain the declared seed "username"/);
  });
});

describe("assembleEffects", () => {
  it("marks openidm, http and logs unobserved instead of treating [] as absence", () => {
    const effects = assembleEffects({
      given: { sharedState: { username: "alice" } },
      dump: {
        outcome: "true",
        before: { username: "alice" },
        final: { username: "alice", verified: true },
      },
      callbacks: [],
    });
    expect(effects.openidm).toEqual([]);
    expect(effects.http).toEqual([]);
    expect(effects.logs).toEqual([]);
    expect(effects.evidence?.unobservedChannels).toEqual([
      "openidm",
      "http",
      "logs",
    ]);
  });

  it("marks state unobserved when callback suspension prevents both snapshots", () => {
    const effects = assembleEffects({
      given: { sharedState: { username: "alice" } },
      callbacks: [{ type: "NameCallback", prompt: "User Name" }],
    });
    expect(effects.outcome).toBeNull();
    expect(effects.sharedState).toEqual({
      initial: { username: "alice" },
      final: { username: "alice" },
    });
    expect(effects.evidence?.unobservedChannels).toEqual([
      "sharedState",
      "transientState",
      "secureState",
      "openidm",
      "http",
      "logs",
    ]);
  });

  it("parses the before and final subject snapshots", () => {
    expect(
      parseSubjectDump(
        JSON.stringify({
          outcome: "true",
          before: { username: "alice" },
          final: { username: "alice", verified: true },
        })
      )
    ).toEqual({
      outcome: "true",
      before: { username: "alice" },
      final: { username: "alice", verified: true },
    });
  });
});
