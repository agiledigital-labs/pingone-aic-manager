import { describe, expect, it } from "vitest";
import { assembleEffects, classifyFinal, parseSubjectDump } from "../../src/aic/record.ts";
import { judge } from "../../src/case/index.ts";
import { caseWith } from "./helpers.ts";

describe("classifyFinal", () => {
  it("keeps given keys in their original bucket and treats new keys as shared", () => {
    const buckets = classifyFinal(
      {
        sharedState: { username: "alice" },
        transientState: { password: "secret" },
      },
      {
        username: "alice",
        password: "secret",
        verified: true,
      }
    );
    expect(buckets.sharedState).toEqual({
      initial: { username: "alice" },
      final: { username: "alice", verified: true },
    });
    expect(buckets.transientState).toEqual({
      initial: { password: "secret" },
      final: { password: "secret" },
    });
    expect(buckets.secureState).toEqual({ initial: {}, final: {} });
  });

  it("records a given key missing from the dump as removed, not as unchanged", () => {
    // Discriminating: copying initial into final would hide the removal.
    const buckets = classifyFinal({ sharedState: { leftover: 1, keep: 2 } }, { keep: 2 });
    expect(buckets.sharedState.final).toEqual({ keep: 2 });
    expect(Object.prototype.hasOwnProperty.call(buckets.sharedState.final, "leftover")).toBe(
      false
    );
  });
});

describe("assembleEffects", () => {
  it("fills every RecordedEffects channel, including empty openidm/http/logs", () => {
    const effects = assembleEffects({
      given: { sharedState: { username: "alice" } },
      dump: {
        outcome: "true",
        before: { username: "alice" },
        final: { username: "alice", verified: true },
      },
      callbacks: [],
    });
    expect(effects.outcome).toBe("true");
    expect(effects.openidm).toEqual([]);
    expect(effects.http).toEqual([]);
    expect(effects.logs).toEqual([]);
    expect(effects.callbacks).toEqual([]);
    expect(Object.keys(effects).sort()).toEqual([
      "callbacks",
      "http",
      "logs",
      "openidm",
      "outcome",
      "secureState",
      "sharedState",
      "transientState",
    ]);
  });

  it("does not invent removals when the result node never ran", () => {
    const effects = assembleEffects({
      given: { sharedState: { username: "alice" } },
      callbacks: [{ type: "NameCallback", prompt: "User Name" }],
    });
    expect(effects.outcome).toBeNull();
    expect(effects.sharedState).toEqual({
      initial: { username: "alice" },
      final: { username: "alice" },
    });
    expect(effects.callbacks).toEqual([{ type: "NameCallback", prompt: "User Name" }]);
  });

  it("is judged by verdict.ts, not by the dump", () => {
    const kase = caseWith({
      given: { sharedState: { username: "alice" } },
      expect: { outcome: "true", sharedState: { added: { verified: true } } },
    });
    const effects = assembleEffects({
      given: kase.given,
      dump: parseSubjectDump({
        outcome: "true",
        before: { username: "alice" },
        final: { username: "alice", verified: true },
      }),
      callbacks: [],
    });
    expect(judge(kase, effects)).toMatchObject({ pass: true, portable: true });
  });
});
