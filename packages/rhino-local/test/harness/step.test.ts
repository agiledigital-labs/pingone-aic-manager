import { describe, expect, it } from "vitest";
import { carryGiven, submittedCallbacks } from "../../src/harness/step.ts";
import type { Given, RecordedEffects } from "../../src/case/types.ts";

function effects(over: Partial<RecordedEffects> = {}): RecordedEffects {
  return {
    outcome: null,
    sharedState: { initial: {}, final: {} },
    transientState: { initial: {}, final: {} },
    secureState: { initial: {}, final: {} },
    callbacks: [],
    openidm: [],
    http: [],
    logs: [],
    ...over,
  };
}

describe("carryGiven", () => {
  const previous: Given = {
    sharedState: { username: "alice" },
    transientState: { otp: "123456" },
    requestHeaders: { "x-forwarded-for": ["203.0.113.7"] },
    existingSession: { tier: "gold" },
    esv: { threshold: "0.8" },
  };

  it("carries shared state as the pass left it", () => {
    const next = carryGiven(
      previous,
      effects({
        sharedState: { initial: { username: "alice" }, final: { username: "alice", stage: "2" } },
      }),
      []
    );
    expect(next.sharedState).toEqual({ username: "alice", stage: "2" });
  });

  it("drops transient state, because the tenant does", () => {
    // The discriminating assertion. Measured 2026-09-14: a value put with
    // nodeState.putTransient reads back null on the resumed pass and is gone
    // from nodeState.keys() — it is not promoted to secure state. An
    // implementation that carried it would let a script that stashes a secret
    // in transient state across a callback pass green here and fail on AIC.
    const next = carryGiven(
      previous,
      effects({
        transientState: { initial: { otp: "123456" }, final: { otp: "123456", pin: "99" } },
      }),
      []
    );
    expect(next.transientState).toBeUndefined();
    expect("transientState" in next).toBe(false);
  });

  it("never sets resumedFromSuspend", () => {
    // Measured false on a callback resume: resumedFromSuspend belongs to
    // action.suspend(). Setting it would also put the local lane somewhere the
    // AIC lane refuses to go (aicUnsupportedReason).
    const next = carryGiven(previous, effects(), []);
    expect(next.resumedFromSuspend).toBeUndefined();
  });

  it("keeps the request channels and the session", () => {
    const next = carryGiven(previous, effects(), []);
    expect(next.requestHeaders).toEqual({ "x-forwarded-for": ["203.0.113.7"] });
    expect(next.existingSession).toEqual({ tier: "gold" });
    expect(next.esv).toEqual({ threshold: "0.8" });
  });

  it("carries managed records the pass created", () => {
    const next = carryGiven(
      previous,
      effects({ managedStore: { "managed/alpha_user": [{ _id: "bob" }] } }),
      []
    );
    expect(next.managed).toEqual({ "managed/alpha_user": [{ _id: "bob" }] });
  });

  it("copies rather than aliases the harvested state", () => {
    const harvested = effects({
      sharedState: { initial: {}, final: { stage: "2" } },
      managedStore: { "managed/alpha_user": [{ _id: "bob" }] },
    });
    const next = carryGiven(previous, harvested, []);
    (next.sharedState as Record<string, unknown>).stage = "mutated";
    (next.managed as Record<string, Array<Record<string, unknown>>>)["managed/alpha_user"]![0]!._id =
      "mutated";
    expect(harvested.sharedState.final).toEqual({ stage: "2" });
    expect(harvested.managedStore).toEqual({ "managed/alpha_user": [{ _id: "bob" }] });
  });

  it("seeds the submitted callbacks for the next pass", () => {
    const next = carryGiven(previous, effects(), [
      { type: "NameCallback", value: "alice" },
    ]);
    expect(next.callbacks).toEqual([{ type: "NameCallback", value: "alice" }]);
  });
});

describe("submittedCallbacks", () => {
  it("attaches replies to emitted callbacks by type, in order", () => {
    const out = submittedCallbacks(
      [
        { type: "NameCallback", prompt: "User Name" },
        { type: "PasswordCallback", prompt: "Password" },
      ],
      [
        { type: "PasswordCallback", value: "hunter2" },
        { type: "NameCallback", value: "alice" },
      ],
      "case"
    );
    expect(out).toEqual([
      { type: "NameCallback", prompt: "User Name", value: "alice" },
      { type: "PasswordCallback", prompt: "Password", value: "hunter2" },
    ]);
  });

  it("fills repeats of one type in declaration order", () => {
    const out = submittedCallbacks(
      [
        { type: "NameCallback", prompt: "first" },
        { type: "NameCallback", prompt: "second" },
      ],
      [
        { type: "NameCallback", value: "one" },
        { type: "NameCallback", value: "two" },
      ],
      "case"
    );
    expect(out.map((cb) => cb.value)).toEqual(["one", "two"]);
  });

  it("submits an unreplied callback with no value, not its outbound one", () => {
    // A HiddenValueCallback carries the value the script sent OUT. Submitting
    // that back would be a mock nobody measured — AM answers an untouched one
    // with its `id`. Leaving it unset makes a script that reads it fail by
    // name instead.
    const out = submittedCallbacks(
      [{ type: "HiddenValueCallback", id: "nonce", value: "abc" }],
      [],
      "case"
    );
    expect(out).toEqual([{ type: "HiddenValueCallback", id: "nonce" }]);
  });

  it("refuses a reply the pass never asked for", () => {
    expect(() =>
      submittedCallbacks(
        [{ type: "NameCallback", prompt: "User Name" }],
        [
          { type: "NameCallback", value: "alice" },
          { type: "PasswordCallback", value: "hunter2" },
        ],
        "resolve [step 1]"
      )
    ).toThrow(/1 unused PasswordCallback reply \(the pass emitted 0\)/);
  });

  it("refuses more replies of a type than the pass emitted", () => {
    expect(() =>
      submittedCallbacks(
        [{ type: "NameCallback", prompt: "User Name" }],
        [
          { type: "NameCallback", value: "alice" },
          { type: "NameCallback", value: "bob" },
        ],
        "resolve [step 1]"
      )
    ).toThrow(/1 unused NameCallback reply \(the pass emitted 1\)/);
  });
});
