import { describe, expect, it } from "vitest";
import { judge } from "../../src/case/index.ts";
import { bucket, makeCase, makeEffects } from "./helpers.ts";

describe("verdict engine — outcome", () => {
  it("passes when the engine-neutral outcome string matches", () => {
    const kase = makeCase({
      name: "next-gen or legacy, same assertion",
      expect: { outcome: "true" },
    });
    const verdict = judge(kase, makeEffects({ outcome: "true" }));
    expect(verdict).toEqual({
      pass: true,
      portable: true,
      mismatches: [],
      summary: "",
    });
  });

  it("fails when the outcome differs", () => {
    const kase = makeCase({
      name: "denied",
      expect: { outcome: "true" },
    });
    const verdict = judge(kase, makeEffects({ outcome: "false" }));
    expect(verdict.pass).toBe(false);
    expect(verdict.mismatches).toEqual([
      {
        channel: "outcome",
        path: "outcome",
        expected: '"true"',
        actual: '"false"',
        message: 'outcome: expected "true", actual "false"',
      },
    ]);
  });

  it("fails when the script produced no outcome", () => {
    const kase = makeCase({ expect: { outcome: "true" } });
    const verdict = judge(kase, makeEffects({ outcome: null }));
    expect(verdict.mismatches).toEqual([
      {
        channel: "outcome",
        path: "outcome",
        expected: '"true"',
        actual: "<no outcome>",
        message: 'outcome: expected "true", script produced no outcome',
      },
    ]);
  });
});

describe("verdict engine — state diffs", () => {
  it("passes when leftover keys from previous nodes stay put (not whole-state equality)", () => {
    // Discriminating: whole-state equality against { verified: true } — or
    // against the expect object itself — would fail because username and
    // leftover are still in final state. A diff only cares that verified
    // was added.
    const initial = {
      username: "alice",
      leftover: "from-previous-node",
    };
    const kase = makeCase({
      given: { sharedState: initial },
      expect: {
        outcome: "true",
        sharedState: { added: { verified: true } },
      },
    });
    const verdict = judge(
      kase,
      makeEffects({
        sharedState: bucket(initial, {
          username: "alice",
          leftover: "from-previous-node",
          verified: true,
        }),
      })
    );
    expect(verdict.pass).toBe(true);
    expect(verdict.mismatches).toEqual([]);
  });

  it("fails when an expected added key was actually changed", () => {
    const kase = makeCase({
      expect: {
        outcome: "true",
        sharedState: { added: { verified: true } },
      },
    });
    const verdict = judge(
      kase,
      makeEffects({
        sharedState: bucket({ verified: false }, { verified: true }),
      })
    );
    expect(verdict.pass).toBe(false);
    expect(verdict.mismatches).toEqual([
      {
        channel: "sharedState",
        path: "verified",
        expected: "added true",
        actual: "changed to true",
        message:
          'sharedState: "verified" was changed, not added (already present in initial state)',
      },
    ]);
  });

  it("fails when an expected added key is missing", () => {
    const kase = makeCase({
      expect: {
        outcome: "true",
        sharedState: { added: { verified: true } },
      },
    });
    const verdict = judge(kase, makeEffects());
    expect(verdict.mismatches).toEqual([
      {
        channel: "sharedState",
        path: "verified",
        expected: "true",
        actual: "<absent>",
        message: 'sharedState: missing added "verified"',
      },
    ]);
  });

  it("fails when an added value differs", () => {
    const kase = makeCase({
      expect: {
        outcome: "true",
        sharedState: { added: { verified: true } },
      },
    });
    const verdict = judge(
      kase,
      makeEffects({
        sharedState: bucket({}, { verified: false }),
      })
    );
    expect(verdict.mismatches).toEqual([
      {
        channel: "sharedState",
        path: "verified",
        expected: "true",
        actual: "false",
        message: 'sharedState: added "verified" differed',
      },
    ]);
  });

  it("fails on an undeclared state mutation", () => {
    const kase = makeCase({
      expect: {
        outcome: "true",
        sharedState: { added: { verified: true } },
      },
    });
    const verdict = judge(
      kase,
      makeEffects({
        sharedState: bucket({}, { verified: true, debugFlag: "yes" }),
      })
    );
    expect(verdict.mismatches).toEqual([
      {
        channel: "sharedState",
        path: "debugFlag",
        expected: "(none)",
        actual: '"yes"',
        message: 'sharedState: undeclared added "debugFlag"',
      },
    ]);
  });

  it("asserts changed and removed keys", () => {
    const kase = makeCase({
      expect: {
        outcome: "true",
        sharedState: {
          changed: { count: 2 },
          removed: ["scratch"],
        },
      },
    });
    const verdict = judge(
      kase,
      makeEffects({
        sharedState: bucket({ count: 1, scratch: "x" }, { count: 2 }),
      })
    );
    expect(verdict.pass).toBe(true);
  });

  it("fails when a key was not removed", () => {
    const kase = makeCase({
      expect: {
        outcome: "true",
        sharedState: { removed: ["scratch"] },
      },
    });
    const verdict = judge(
      kase,
      makeEffects({
        sharedState: bucket({ scratch: "x" }, { scratch: "x" }),
      })
    );
    expect(verdict.mismatches).toEqual([
      {
        channel: "sharedState",
        path: "scratch",
        expected: "removed",
        actual: '"x"',
        message: 'sharedState: "scratch" was not removed',
      },
    ]);
  });

  it("still checks listed keys when undeclared state mutations are allowed", () => {
    const kase = makeCase({
      expect: {
        outcome: "true",
        sharedState: { added: { verified: true } },
        allowUndeclared: { sharedState: true },
      },
    });
    const verdict = judge(
      kase,
      makeEffects({
        sharedState: bucket({}, { verified: false, extra: 1 }),
      })
    );
    expect(verdict.mismatches).toEqual([
      {
        channel: "sharedState",
        path: "verified",
        expected: "true",
        actual: "false",
        message: 'sharedState: added "verified" differed',
      },
    ]);
  });
});

describe("verdict engine — effects record", () => {
  it("throws when a required channel is omitted rather than treating it as empty", () => {
    const kase = makeCase();
    const effects = makeEffects() as unknown as Record<string, unknown>;
    delete effects.openidm;
    expect(() => judge(kase, effects)).toThrow(
      /effects is missing openidm — a runner that does not record a channel would silently assert nothing/
    );
  });
});

describe("verdict engine — failure message", () => {
  it("names the case, channel, key, expected and actual", () => {
    const kase = makeCase({
      name: "grants access when the user has a verified email",
      expect: {
        outcome: "true",
        sharedState: { added: { verified: true } },
      },
    });
    const verdict = judge(
      kase,
      makeEffects({
        outcome: "false",
        sharedState: bucket({}, {}),
        openidm: [
          {
            method: "create",
            resource: "managed/alpha_user/alice",
            body: { userName: "alice" },
          },
        ],
      })
    );
    expect(verdict.pass).toBe(false);
    expect(verdict.summary).toBe(
      `"grants access when the user has a verified email" failed (3 mismatches):
outcome: expected "true", actual "false"
  expected: "true"
  actual: "false"
sharedState: missing added "verified"
  expected: true
  actual: <absent>
openidm: undeclared write create managed/alpha_user/alice body={"userName":"alice"}
  expected: (none)
  actual: create managed/alpha_user/alice body={"userName":"alice"}`
    );
  });
});
