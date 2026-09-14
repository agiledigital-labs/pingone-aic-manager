import { describe, expect, it } from "vitest";
import { z } from "zod";
import {
  aicWhenEnabled,
  defineSuite,
  managed,
  useLease,
} from "../../src/harness/index.ts";

const USER_IDS = {
  alice: "00000000-0000-4000-8000-000000000011",
  bob: "00000000-0000-4000-8000-000000000012",
  carol: "00000000-0000-4000-8000-000000000013",
  dave: "00000000-0000-4000-8000-000000000014",
} as const;

const SCRIPT = [
  'var user = openidm.read("managed/alpha_user/" + nodeState.get("userId"));',
  'if (user === null) { action.goTo("notFound"); } else {',
  '  nodeState.putShared("matchedId", user._id);',
  '  logger.info("matched {}", user.userName);',
  '  action.goTo("matched");',
  "}",
].join("\n");

const cleanedInputs: string[] = [];

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
    // Two tenant rules the local mock store does not enforce, both measured
    // 2026-09-14 (docs/api/10-managed-objects.md): every property must be
    // DECLARED on the managed schema, so the locale rides in one of the
    // tenant's generic string slots rather than an invented
    // `preferredLocale`; and `mail`, `givenName` and `sn` are required by
    // policy on alpha_user.
    return fixtures.create("managed/alpha_user", {
      _id: input.userId,
      userName: input.userId,
      mail: `${input.userId}@example.com`,
      givenName: "Fixture",
      sn: input.userId,
      frUnindexedString1: input.locale,
    });
  },
  cleanup: (_idm, { input }) => {
    cleanedInputs.push(input.userId);
    return Promise.resolve();
  },
});

describe("resolve-identity", () => {
  const lease = useLease(suite, {
    timeoutMs: 10_000,
    ...aicWhenEnabled("resolve-identity"),
  });

  it("matches the seeded candidate", async () => {
    const run = await lease
      .run({ userId: USER_IDS.alice })
      .expect({
        outcome: "matched",
        sharedState: { added: { matchedId: USER_IDS.alice } },
        logs: [{ level: "info", message: `matched ${USER_IDS.alice}` }],
        allowUndeclared: { openidmReads: true },
      });
    expect(run.verdict.summary, run.verdict.summary).toBe("");
  });

  it("applies declared defaults and the always channels", async () => {
    const run = await lease
      .run({ userId: USER_IDS.bob })
      .check((_idm, { input }) => {
        expect(input).toEqual({ userId: USER_IDS.bob, locale: "en-AU" });
      })
      .expect({
        outcome: "matched",
        sharedState: { added: { matchedId: USER_IDS.bob } },
        allowUndeclared: { openidmReads: true, logs: true },
      });

    // `always` state, the ESV override in its state form, the beforeRun
    // addition and the parsed inputs all arrive through one channel.
    expect(run.kase.given.sharedState).toMatchObject({
      realmName: "alpha",
      "esv.idr.match.threshold": "0.80",
      correlationId: `c-${USER_IDS.bob}`,
      userId: USER_IDS.bob,
      locale: "en-AU",
    });
  });

  it("sees the suite fixture and its own, and not another test's", async () => {
    const run = await lease
      .run({ userId: USER_IDS.carol })
      .check(async (idm) => {
        expect(await idm.read("managed/alpha_role/idr-reviewer")).not.toBeNull();
        expect(
          await idm.read(`managed/alpha_user/${USER_IDS.carol}`)
        ).not.toBeNull();
        // alice belonged to the first test; afterEach dropped her.
        expect(
          await idm.read(`managed/alpha_user/${USER_IDS.alice}`)
        ).toBeNull();
      })
      .expect({
        outcome: "matched",
        sharedState: { added: { matchedId: USER_IDS.carol } },
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
      .run({ userId: USER_IDS.dave })
      .expect({
        outcome: "matched",
        sharedState: { added: { matchedId: USER_IDS.dave } },
        allowUndeclared: { openidmReads: true, logs: true },
      });
    expect(run.kase.outcomes).toEqual(["matched", "notFound"]);
    expect(run.verdict.summary, run.verdict.summary).toBe("");
  });

  it("runs cleanup when a final check throws", async () => {
    const before = cleanedInputs.length;
    await expect(
      lease
        .run({ userId: USER_IDS.dave })
        .check(() => {
          throw new Error("deliberate check failure");
        })
        .expect({
          outcome: "matched",
          allowUndeclared: { openidmReads: true, logs: true, sharedState: true },
        })
    ).rejects.toThrow(/deliberate check failure/);
    expect(cleanedInputs.slice(before)).toEqual([USER_IDS.dave]);
  });
});
