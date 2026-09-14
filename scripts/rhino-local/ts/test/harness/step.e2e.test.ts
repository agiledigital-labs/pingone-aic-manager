import { describe, expect, it } from "vitest";
import { defineSuite, useLease } from "../../src/harness/index.ts";

/**
 * A two-pass journey in one node: ask for a name, then use it.
 *
 * `scratch` is deliberately transient and read back on the resumed pass. It
 * must come back `null` — measured 2026-09-14 on a live tenant, transient
 * state does not survive a callback round trip.
 */
const SCRIPT = [
  "if (callbacks.isEmpty()) {",
  '  openidm.create("managed/alpha_user", "pending", {',
  '    _id: "pending", userName: "pending", status: "awaiting-name"',
  "  });",
  '  nodeState.putShared("stage", "asked");',
  '  nodeState.putTransient("scratch", "vanishes");',
  '  callbacksBuilder.nameCallback("User Name");',
  "} else {",
  "  var name = String(callbacks.getNameCallbacks().get(0));",
  '  var pending = openidm.read("managed/alpha_user/pending");',
  '  nodeState.putShared("username", name);',
  '  nodeState.putShared("carriedStatus", String(pending.status));',
  '  nodeState.putShared("scratchOnResume", String(nodeState.get("scratch")));',
  '  action.goTo("done");',
  "}",
].join("\n");

const suite = defineSuite({
  name: "ask-for-a-name",
  script: SCRIPT,
  outcomes: ["done"],
  cleanup: async (idm) => {
    await idm.delete("managed/alpha_user/pending");
  },
});

describe("step chain", () => {
  const lease = useLease(suite, { timeoutMs: 10_000 });

  it("carries state, the store and the reply across a suspend", async () => {
    let seenInStep = "";
    const run = await lease
      .run()
      .step({
        expect: {
          callbacks: [{ type: "NameCallback", prompt: "User Name" }],
          sharedState: { added: { stage: "asked" } },
          transientState: { added: { scratch: "vanishes" } },
          allowUndeclared: { openidmWrites: true },
        },
        check: async (idm, ctx) => {
          expect(ctx.step).toBe(1);
          const pending = await idm.read("managed/alpha_user/pending");
          seenInStep = String(pending?.status);
        },
        reply: [{ type: "NameCallback", value: "alice" }],
      })
      .expect({
        outcome: "done",
        sharedState: {
          added: {
            username: "alice",
            carriedStatus: "awaiting-name",
            // The record the first pass created is still there…
            scratchOnResume: "null",
            // …and the transient value it stashed is not.
          },
        },
        allowUndeclared: { openidmReads: true },
      });

    expect(seenInStep).toBe("awaiting-name");
    expect(run.verdict.summary, run.verdict.summary).toBe("");
    expect(run.steps).toHaveLength(1);
    expect(run.steps[0]?.kase.name).toMatch(/\[step 1\]$/);
    expect(run.steps[0]?.submitted).toEqual([
      { type: "NameCallback", prompt: "User Name", value: "alice" },
    ]);
    // The second pass was seeded from the first, not from the request.
    expect(run.kase.given.sharedState).toMatchObject({ stage: "asked" });
    expect(run.kase.given.transientState).toBeUndefined();
    expect(run.kase.given.callbacks).toEqual([
      { type: "NameCallback", prompt: "User Name", value: "alice" },
    ]);
  });

  it("a reply function sees what the pass emitted", async () => {
    const run = await lease
      .run()
      .step({
        reply: (ctx) => [
          { type: "NameCallback", value: `from-${String(ctx.callbacks[0]?.prompt)}` },
        ],
        expect: {
          allowUndeclared: {
            openidmWrites: true,
            callbacks: true,
            sharedState: true,
            transientState: true,
          },
        },
      })
      .expect({
        outcome: "done",
        sharedState: {
          added: {
            username: "from-User Name",
            carriedStatus: "awaiting-name",
            scratchOnResume: "null",
          },
        },
        allowUndeclared: { openidmReads: true },
      });
    expect(run.verdict.summary, run.verdict.summary).toBe("");
  });

  it("steps() is the same chain, built from data", async () => {
    const run = await lease
      .run()
      .steps([
        {
          reply: [{ type: "NameCallback", value: "carol" }],
          expect: {
            allowUndeclared: {
              openidmWrites: true,
              callbacks: true,
              sharedState: true,
              transientState: true,
            },
          },
        },
      ])
      .expect({
        outcome: "done",
        sharedState: {
          added: {
            username: "carol",
            carriedStatus: "awaiting-name",
            scratchOnResume: "null",
          },
        },
        allowUndeclared: { openidmReads: true },
      });
    expect(run.steps).toHaveLength(1);
    expect(run.verdict.summary, run.verdict.summary).toBe("");
  });

  it("aborts the chain when a step's expectation fails, naming the step", async () => {
    await expect(
      lease
        .run()
        .step({
          expect: {
            callbacks: [{ type: "PasswordCallback" }],
            allowUndeclared: { openidmWrites: true, sharedState: true, transientState: true },
          },
          reply: [{ type: "NameCallback", value: "dave" }],
        })
        .expect({ outcome: "done", allowUndeclared: { openidmReads: true, sharedState: true } })
    ).rejects.toThrow(/\[step 1\][\s\S]*callbacks/);
  });

  it("reports a pass that decided instead of suspending", async () => {
    // Two steps, but the script only suspends once: the second pass reaches
    // `done` and has no callbacks to reply to.
    await expect(
      lease
        .run()
        .steps([
          {
            reply: [{ type: "NameCallback", value: "erin" }],
            expect: {
              allowUndeclared: {
                openidmWrites: true,
                callbacks: true,
                sharedState: true,
                transientState: true,
              },
            },
          },
          {
            reply: [],
            expect: { allowUndeclared: { sharedState: true, openidmReads: true } },
          },
        ])
        .expect({ outcome: "done", allowUndeclared: { sharedState: true, openidmReads: true } })
    ).rejects.toThrow(/\[step 2\][\s\S]*suspend with callbacks and decide nothing/);
  });
});
