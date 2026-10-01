import { describe, expect, it } from "vitest";
import { randomUUID } from "node:crypto";
import { z } from "zod";
import { chainFromRunResult, conformChain } from "../../src/aic/conform.ts";
import { conform } from "../../src/aic/conform.ts";
import { oneShotSubjectId, oneShotSubjectName } from "../../src/aic/emit-journey.ts";
import { runCase } from "../../src/bindings/index.ts";
import { RhinoRunner } from "../../src/runner.ts";
import { caseWith } from "../aic/helpers.ts";
import {
  aicWhenEnabled,
  AIC_LANE_ENV,
  defineSuite,
  managed,
  localIdmHandle,
  useLease,
} from "../../src/harness/index.ts";
import type { LeaseLaneHooks, RunResultInput } from "../../src/harness/index.ts";
import type { RecordedEffects } from "../../src/case/index.ts";

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
  const aic = aicWhenEnabled("resolve-identity");
  // AIC adds subject PUT, confirming GET, authenticate, and fixture I/O to
  // a run whose Rhino evaluation alone may use the 10-second budget below.
  const testTimeoutMs = aic.aic === undefined ? 5_000 : 20_000;
  const lease = useLease(suite, {
    timeoutMs: 10_000,
    ...aic,
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
  }, testTimeoutMs);

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

    // State, beforeRun and parsed inputs share a channel; ESVs use systemEnv.
    expect(run.kase.given.sharedState).toMatchObject({
      realmName: "alpha",
      correlationId: `c-${USER_IDS.bob}`,
      userId: USER_IDS.bob,
      locale: "en-AU",
    });
    expect(run.kase.given.esv).toEqual({ "esv.idr.match.threshold": "0.80" });
  }, testTimeoutMs);

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
  }, testTimeoutMs);

  it("names the missing required input before anything runs", async () => {
    await expect(
      // @ts-expect-error userId is required, which is the point
      lease.run({}).expect({ outcome: "matched" })
    ).rejects.toThrow(/userId/);
  }, testTimeoutMs);

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
  }, testTimeoutMs);

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
  }, testTimeoutMs);
});

const esvSuite = defineSuite({
  name: "system-env-esv",
  script: [
    'var value = systemEnv.getProperty("esv.rl.example");',
    'action.goTo(value === "override" ? "override" : "other");',
  ].join("\n"),
  outcomes: ["override", "other"],
  always: { esv: { "rl.example": "suite" } },
  beforeRun: ({ request }) => {
    if (request.esv["rl.example"] === "suite") {
      request.esv["rl.example"] = "before";
    }
  },
});

describe("system-env-esv", () => {
  const lease = useLease(esvSuite);

  // Regression: the ESV channel used to seed only shared state.
  it("reads a per-test ESV override through systemEnv", async () => {
    const run = await lease.run().esv({ "rl.example": "override" }).expect({ outcome: "override" });
    expect(run.verdict.pass).toBe(true);
    expect(run.kase.given.sharedState).toBeUndefined();
  });

  it("can mirror ESV declarations into shared state", async () => {
    const run = await lease.run().esv({ "rl.example": "override" }).esvInState()
      .expect({ outcome: "override" });
    expect(run.kase.given.sharedState).toEqual({ "esv.rl.example": "override" });
  });

  it("lets beforeRun change the suite ESV declaration", async () => {
    const run = await lease.run().expect({ outcome: "other" });
    expect(run.kase.given.esv).toEqual({ "esv.rl.example": "before" });
  });

  it("names the fix for an already-prefixed per-test ESV key", async () => {
    await expect(
      lease.run().esv({ "esv.rl.example": "override" }).expect({ outcome: "override" })
    ).rejects.toThrow(/use "rl\.example" for systemEnv/);
  });
});

const namedSuite = defineSuite({
  name: "local script name",
  scriptName: "explicit-local-name",
  script: 'action.goTo(scriptName === "explicit-local-name" ? "named" : "wrong");',
  outcomes: ["named", "wrong"],
});

describe("local script name", () => {
  const lease = useLease(namedSuite);

  // Regression: the harness used to leave scriptName at the mock sentinel.
  it("uses the suite's explicit scriptName", async () => {
    const run = await lease.run().expect({ outcome: "named" });
    expect(run.kase.given.scriptName).toBe("explicit-local-name");
    expect(run.verdict.pass).toBe(true);
  });
});

const loggerNameSuite = defineSuite({
  name: "logger-name",
  script: 'logger.info("{}", logger.getName()); action.goTo("done");',
  outcomes: ["done"],
});

describe("logger-name", () => {
  const lease = useLease(loggerNameSuite);

  // Regression: logger.getName returned only scriptName locally.
  it("uses the measured decision-node format with a placeholder ID", async () => {
    const run = await lease.run().expect({
      outcome: "done",
      logs: [{ level: "info", message: "scripts.AUTHENTICATION_TREE_DECISION_NODE.<unseeded-script-id>.(logger-name)" }],
    });
    expect(run.verdict.pass).toBe(true);
  });
});

const bridgeSuite = defineSuite({
  name: "lease-chain-bridge",
  script: 'action.goTo("done");',
  outcomes: ["done"],
});

describe("lease result to one-shot conformance", () => {
  const lease = useLease(bridgeSuite);

  it("invokes the AIC runner for a plain leased script", async () => {
    const result = await lease.run().expect({ outcome: "done" });
    let calls = 0;
    const report = await conformChain({
      ...chainFromRunResult(result),
      source: bridgeSuite.spec.script,
      aic: ({ cases }) => {
        calls += 1;
        expect(cases[0]?.given.scriptName).toBeUndefined();
        return Promise.resolve([result.effects]);
      },
    });
    expect(calls).toBe(1);
    expect(report.passes[0]?.aic.skipped).toBeUndefined();
  });

  it("seeds a one-shot identity before local execution", async () => {
    const runner = await RhinoRunner.spawn();
    const runId = "leasebridge01";
    const oneShotLease = bridgeSuite.lease({ runner, oneShotRunId: runId });
    try {
      oneShotLease.open();
      const result = await oneShotLease.run().expect({ outcome: "done" });
      expect(result.kase.given.scriptName).toBe(oneShotSubjectName(runId));
      expect(result.kase.given.loggerScriptId).toBe(oneShotSubjectId(runId));
      const report = await conformChain({
        ...chainFromRunResult(result),
        source: bridgeSuite.spec.script,
        aic: ({ cases, runId: actualRunId }) => {
          expect(cases[0]?.given.scriptName).toBeUndefined();
          expect(cases[0]?.given.loggerScriptId).toBeUndefined();
          expect(actualRunId).toBe(runId);
          return Promise.resolve([result.effects]);
        },
      });
      expect(report.passes[0]?.aic.skipped).toBeUndefined();
    } finally {
      await oneShotLease.close();
      await runner.close();
    }
  });

  it.skipIf(process.env[AIC_LANE_ENV] !== "1")(
    "compares a one-shot lease result with the tenant",
    async () => {
      const runner = await RhinoRunner.spawn();
      const oneShotLease = bridgeSuite.lease({
        runner,
        oneShotRunId: randomUUID().replace(/-/g, "").slice(0, 12),
      });
      try {
        oneShotLease.open();
        const result = await oneShotLease.run().expect({ outcome: "done" });
        const report = await conformChain({
          ...chainFromRunResult(result),
          source: bridgeSuite.spec.script,
          aic: "tenant",
        });
        expect(report.passes[0]?.aic.verdict?.pass).toBe(true);
        expect(report.disagreements).toEqual([]);
      } finally {
        await oneShotLease.close();
        await runner.close();
      }
    },
    30_000
  );
});

describe("unsupported one-shot identity", () => {
  it("preserves an author logger ID for local execution", async () => {
    const runner = await RhinoRunner.spawn();
    const kase = caseWith({
      given: { loggerScriptId: "caller-id" },
      script: 'action.goTo(logger.getName().indexOf(".caller-id.") >= 0 ? "bad" : "good");',
      expect: { outcome: "good" },
    });
    try {
      const report = await conform({
        kase,
        source: kase.script,
        local: async ({ kase: localCase }) => (await runCase(runner, localCase)).effects,
        aic: "tenant",
      });
      expect(report.local.effects?.outcome).toBe("bad");
      expect(report.local.verdict?.pass).toBe(false);
      expect(report.aic.skipped).toMatch(/given\.loggerScriptId/);
    } finally {
      await runner.close();
    }
  });
});

// A 0.1.2 consumer hands back effects without the two later channels, both
// through chainFromRunResult and through a lane it implemented itself. Each
// boundary must fill them in and mark them unobserved, never leave them out.
describe("0.1.2-shaped effects at the lane boundaries", () => {
  const lease = useLease(bridgeSuite);
  const strip = (effects: Record<string, unknown>): RecordedEffects => {
    const { sessionProperties: _s, identityWrites: _i, ...rest } = effects;
    return rest as unknown as RecordedEffects;
  };

  it("chainFromRunResult normalises a 0.1.2 result", async () => {
    const result = await lease.run().expect({ outcome: "done" });
    const old: RunResultInput = { ...result, effects: strip({ ...result.effects }) };
    const chain = chainFromRunResult(old);
    expect(chain.localEffects[0]?.sessionProperties).toEqual({ initial: {}, final: {} });
    expect(chain.localEffects[0]?.identityWrites).toEqual([]);
    expect(chain.localEffects[0]?.evidence?.unobservedChannels).toEqual([
      "sessionProperties",
      "identityWrites",
    ]);
  });

  it("a lane's check hook hands the author complete effects", async () => {
    const runner = await RhinoRunner.spawn();
    let hooks: LeaseLaneHooks | undefined;
    const laneLease = bridgeSuite.lease({
      runner,
      lane: {
        run: (request) => {
          hooks = request.hooks;
          return Promise.reject(new Error("lane stops here"));
        },
        endTest: () => Promise.resolve(),
      },
    });
    const seen: unknown[] = [];
    try {
      laneLease.open();
      await expect(
        laneLease
          .run()
          .check((_idm, { effects }) => {
            seen.push(effects.identityWrites, effects.evidence?.unobservedChannels);
          })
          .expect({ outcome: "done" })
      ).rejects.toThrow(/lane stops here/);
      seen.length = 0;
      const check = hooks?.finalChecks[0];
      expect(check).toBeDefined();
      const result = await lease.run().expect({ outcome: "done" });
      await check?.(localIdmHandle({}, () => undefined), strip({ ...result.effects }));
      expect(seen).toEqual([[], ["sessionProperties", "identityWrites"]]);
    } finally {
      await laneLease.close();
      await runner.close();
    }
  });
});
