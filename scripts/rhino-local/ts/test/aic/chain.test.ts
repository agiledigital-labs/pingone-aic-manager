import { describe, expect, it } from "vitest";
import { fillCallbackInputs } from "../../src/aic/callbacks.ts";
import { runAicChain } from "../../src/aic/run.ts";
import { caseWith } from "./helpers.ts";
import { mockChain } from "./mock-chain.ts";
const SCRIPT = [
  "if (callbacks.isEmpty()) {",
  '  nodeState.putShared("stage", "asked");',
  '  callbacksBuilder.nameCallback("User Name");',
  "} else {",
  '  action.goTo("done");',
  "}",
].join("\n");

describe("fillCallbackInputs", () => {
  const body = {
    authId: "jwt",
    callbacks: [
      {
        type: "NameCallback",
        output: [{ name: "prompt", value: "User Name" }],
        input: [{ name: "IDToken1", value: "" }],
      },
    ],
  };

  it("fills the input slot and keeps everything else", () => {
    const out = JSON.parse(
      fillCallbackInputs(body, [{ type: "NameCallback", value: "alice" }], "step 1")
    ) as typeof body;
    expect(out.authId).toBe("jwt");
    expect(out.callbacks[0]?.input[0]?.value).toBe("alice");
    expect(out.callbacks[0]?.output[0]?.value).toBe("User Name");
    // The response handed in is not edited in place.
    expect(body.callbacks[0]?.input[0]?.value).toBe("");
  });

  it("leaves an unanswered callback holding AM's own default", () => {
    const hidden = {
      callbacks: [
        {
          type: "HiddenValueCallback",
          output: [{ name: "value", value: "sent-out" }],
          input: [{ name: "IDToken1", value: "the-id" }],
        },
      ],
    };
    const out = JSON.parse(fillCallbackInputs(hidden, [], "step 1")) as typeof hidden;
    expect(out.callbacks[0]?.input[0]?.value).toBe("the-id");
  });

  it("refuses a reply the pass never asked for", () => {
    expect(() =>
      fillCallbackInputs(body, [{ type: "PasswordCallback", value: "x" }], "step 1")
    ).toThrow(/1 unused PasswordCallback/);
  });
});

describe("runAicChain", () => {
  it("re-enters the same node, seeding only the first pass", async () => {
    const fake = mockChain({ before: { username: "alice", stage: "asked" } });
    const first = caseWith({
      name: "ask",
      given: { sharedState: { username: "alice" } },
      expect: { outcome: null, callbacks: [{ type: "NameCallback" }] },
    });
    const second = caseWith({
      name: "answer",
      given: { sharedState: { username: "alice", stage: "asked" } },
      expect: { outcome: "done" },
    });
    const passes = await runAicChain([first, second], SCRIPT, {
      io: fake.io,
      runId: "chain1",
      project: "/tmp/rhino-local-aic-test",
      replies: [[{ type: "NameCallback", value: "alice" }]],
    });

    expect(passes).toHaveLength(2);
    expect(passes[0]?.outcome).toBeNull();
    expect(passes[0]?.callbacks).toEqual([{ type: "NameCallback", prompt: "User Name" }]);
    expect(passes[1]?.outcome).toBe("done");

    // One journey, two authenticate calls, the second carrying the answer.
    expect(fake.authPosts).toHaveLength(2);
    expect(fake.authPosts[0]).toBe("{}");
    const submitted = JSON.parse(String(fake.authPosts[1])) as {
      authId: string;
      callbacks: Array<{ input: Array<{ value: string }> }>;
    };
    expect(submitted.authId).toBe("jwt-1");
    expect(submitted.callbacks[0]?.input[0]?.value).toBe("alice");
    expect(fake.treesCreated).toEqual(["rl-aic-chain1"]);

    // The subject seeds behind `callbacks.isEmpty()`, so the resumed pass
    // keeps what the first pass left instead of being reset to the request.
    expect(fake.subjectSource).toMatch(/if \(callbacks\.isEmpty\(\)\) \{\n\s+__rhinoLocalHarness_chain1\.seed/);
  });

  it("declares every outcome any pass may reach", async () => {
    const fake = mockChain();
    await runAicChain(
      [
        caseWith({ name: "ask", expect: { outcome: null } }),
        caseWith({ name: "answer", expect: { outcome: "done" } }),
      ],
      SCRIPT,
      {
        io: fake.io,
        runId: "chain2",
        project: "/tmp/rhino-local-aic-test",
        replies: [[{ type: "NameCallback", value: "alice" }]],
      }
    );
    // A tenant answers an undeclared outcome with a bare 401, so "done" has to
    // be on the subject node even though the FIRST pass never mentions it.
    expect(fake.subjectOutcomes).toEqual(["true", "false", "done"]);
  });

  it("refuses a reply count that does not match the passes", async () => {
    const fake = mockChain();
    await expect(
      runAicChain([caseWith({ name: "one" })], SCRIPT, {
        io: fake.io,
        runId: "chain3",
        project: "/tmp/rhino-local-aic-test",
        replies: [[{ type: "NameCallback", value: "alice" }]],
      })
    ).rejects.toThrow(/1 passes need 0 reply sets, got 1/);
  });

  it("says so when the journey finishes before a declared step", async () => {
    const fake = mockChain({ finishImmediately: true });
    await expect(
      runAicChain(
        [
          caseWith({ name: "ask", expect: { outcome: null } }),
          caseWith({ name: "answer", expect: { outcome: "done" } }),
        ],
        SCRIPT,
        {
          io: fake.io,
          runId: "chain4",
          project: "/tmp/rhino-local-aic-test",
          replies: [[{ type: "NameCallback", value: "alice" }]],
        }
      )
    ).rejects.toThrow(/the journey finished before this step ran/);
  });
});
