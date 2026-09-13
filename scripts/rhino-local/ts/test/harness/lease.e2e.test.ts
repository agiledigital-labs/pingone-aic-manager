import { describe, expect, it } from "vitest";
import { z } from "zod";
import { defineSuite, managed, useLease } from "../../src/harness/index.ts";

const SCRIPT = [
  'var user = openidm.read("managed/alpha_user/" + nodeState.get("userId"));',
  'if (user === null) { action.goTo("notFound"); } else {',
  '  nodeState.putShared("matchedId", user._id);',
  '  logger.info("matched {}", user.userName);',
  '  action.goTo("matched");',
  "}",
].join("\n");

const suite = defineSuite({
  name: "resolve-identity",
  script: SCRIPT,
  outcomes: ["matched", "notFound"],
  inputs: z.object({
    userId: z.string(),
    locale: z.enum(["en-US", "en-AU"]).default("en-AU"),
  }),
  always: {
    state: { shared: { realmName: "alpha" } },
    esv: { "idr.match.threshold": "0.80" },
  },
  fixtures: {
    reviewer: managed("managed/alpha_role", { _id: "idr-reviewer", name: "IDR Reviewer" }),
  },
  beforeRun: ({ input, request, fixtures }) => {
    request.state.shared.correlationId = `c-${input.userId}`;
    return fixtures.create("managed/alpha_user", {
      _id: input.userId,
      userName: input.userId,
      preferredLocale: input.locale,
    });
  },
});

describe("resolve-identity", () => {
  const lease = useLease(suite, { timeoutMs: 10_000 });

  it("matches the seeded candidate", async () => {
    const run = await lease
      .run({ userId: "alice" })
      .expect({
        outcome: "matched",
        sharedState: { added: { matchedId: "alice" } },
        logs: [{ level: "info", message: "matched alice" }],
        allowUndeclared: { openidmReads: true },
      });
    expect(run.verdict.summary, run.verdict.summary).toBe("");
  });

  it("applies declared defaults and the always channels", async () => {
    const run = await lease
      .run({ userId: "bob" })
      .check((_idm, { input }) => {
        expect(input).toEqual({ userId: "bob", locale: "en-AU" });
      })
      .expect({
        outcome: "matched",
        sharedState: { added: { matchedId: "bob" } },
        allowUndeclared: { openidmReads: true, logs: true },
      });

    // `always` state, the ESV override in its state form, the beforeRun
    // addition and the parsed inputs all arrive through one channel.
    expect(run.kase.given.sharedState).toMatchObject({
      realmName: "alpha",
      "esv.idr.match.threshold": "0.80",
      correlationId: "c-bob",
      userId: "bob",
      locale: "en-AU",
    });
  });

  it("sees the suite fixture and its own, and not another test's", async () => {
    const run = await lease
      .run({ userId: "carol" })
      .check(async (idm) => {
        expect(await idm.read("managed/alpha_role/idr-reviewer")).not.toBeNull();
        expect(await idm.read("managed/alpha_user/carol")).not.toBeNull();
        // alice belonged to the first test; afterEach dropped her.
        expect(await idm.read("managed/alpha_user/alice")).toBeNull();
      })
      .expect({
        outcome: "matched",
        sharedState: { added: { matchedId: "carol" } },
        allowUndeclared: { openidmReads: true, logs: true },
      });
    expect(run.verdict.summary, run.verdict.summary).toBe("");
  });

  it("names the missing required input before anything runs", async () => {
    await expect(
      // @ts-expect-error userId is required, which is the point
      lease.run({}).expect({ outcome: "matched" })
    ).rejects.toThrow(/userId/);
  });

  it("reports an undeclared outcome as a configuration fault", async () => {
    const run = await lease
      .run({ userId: "dave" })
      .expect({
        outcome: "matched",
        sharedState: { added: { matchedId: "dave" } },
        allowUndeclared: { openidmReads: true, logs: true },
      });
    expect(run.kase.outcomes).toEqual(["matched", "notFound"]);
    expect(run.verdict.summary, run.verdict.summary).toBe("");
  });
});
