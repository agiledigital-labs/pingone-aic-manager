import { describe, expect, it } from "vitest";
import { defineSuite, useLease } from "../../src/harness/index.ts";
import { mergeChannels, resolveDraft, toGiven } from "../../src/harness/spec.ts";
import type { RequestDraft } from "../../src/harness/types.ts";
import { conform } from "../../src/aic/conform.ts";
import { runScript } from "../bindings/load-behaviour.ts";
import { aicUnsupportedReason } from "../../src/aic/unsupported.ts";
import { isPortable, validateCase } from "../../src/case/index.ts";
import { caseWith } from "../aic/helpers.ts";

const suiteReply = { match: { resource: "endpoint/example", action: "evaluate" }, reply: { body: "suite" } };
const testReply = { ...suiteReply, reply: { body: "test" } };

describe("action and ESV channel plumbing", () => {
  it("merges replies per-test first, clones nested bodies and carries hook edits", () => {
    const draft = mergeChannels({ openidmActions: [suiteReply], esvUndeclared: "absent" }, { openidmActions: [testReply], esvUndeclared: "error" });
    expect(draft.openidmActions).toEqual([testReply, suiteReply]);
    expect(draft.esvUndeclared).toBe("error");
    const first = draft.openidmActions[0];
    if (first) first.reply.body = "hook";
    draft.esvUndeclared = "absent";
    expect(toGiven(draft).openidmActions?.[0]?.reply.body).toBe("hook");
    expect(toGiven(draft).esvUndeclared).toBe("absent");
    expect(testReply.reply.body).toBe("test");
  });

  it("accepts a 0.1.2 draft with both new channels omitted", () => {
    const old: RequestDraft = { state: { shared: {}, transient: {} }, esv: {}, headers: {}, params: {}, session: {}, sessionRequested: false };
    expect(resolveDraft(old).openidmActions).toEqual([]);
    expect(resolveDraft(old).esvUndeclared).toBe("error");
    expect(toGiven(old).openidmActions).toBeUndefined();
    expect(toGiven(old).esvUndeclared).toBeUndefined();
    expect(toGiven(old, { esvUndeclared: "absent" }).esvUndeclared).toBe("absent");
  });

  it("matches HTTP portability and AIC eligibility for action replies", () => {
    const kase = caseWith({ given: { openidmActions: [suiteReply] } });
    expect(isPortable(kase)).toBe(false);
    expect(aicUnsupportedReason(kase)).toBe("given.openidmActions is environment-dependent; AIC lane skips rather than run against whatever the tenant holds");
    expect(aicUnsupportedReason(caseWith({ given: { openidmActions: [] } }))).toBeUndefined();
    expect(isPortable(caseWith({ given: { openidmActions: [] } }))).toBe(true);
  });

  it("reports the same AIC observation gap as HTTP without invoking a tenant runner", async () => {
    const kase = caseWith({ given: { openidmActions: [suiteReply] } });
    const report = await conform({
      kase,
      source: 'action.goTo("true");',
      local: async () => runScript('action.goTo("true");'),
      aic: async () => { throw new Error("tenant runner must not be invoked"); },
    });
    expect(report.aic.skipped).toMatch(/given.openidmActions is environment-dependent/);
    expect(report.observationGaps).toEqual([expect.objectContaining({ channel: "outcome", path: "aic-lane", aic: "ineligible" })]);
  });

  it("keeps the absent ESV policy portable and AIC-eligible", () => {
    const kase = caseWith({ given: { esvUndeclared: "absent" } });
    expect(isPortable(kase)).toBe(true);
    expect(aicUnsupportedReason(kase)).toBeUndefined();
  });

  it("validates both channels and preserves regex matching", () => {
    const given = { openidmActions: [{ ...suiteReply, match: { resource: /endpoint\/.*/, action: "evaluate" } }], esvUndeclared: "absent" as const };
    expect(validateCase(caseWith({ given })).given).toEqual(given);
  });

  it.each([
    { esvUndeclared: true },
    { openidmActions: {} },
    { openidmActions: [{ ...suiteReply, extra: true }] },
    { openidmActions: [{ ...suiteReply, match: { resource: "endpoint/example", action: 1 } }] },
    { openidmActions: [{ ...suiteReply, match: { resource: 1, action: "evaluate" } }] },
    { openidmActions: [{ ...suiteReply, match: { ...suiteReply.match, content: {} } }] },
    { openidmActions: [{ ...suiteReply, reply: {} }] },
    { openidmActions: [{ ...suiteReply, reply: { body: undefined } }] },
    { openidmActions: [{ ...suiteReply, reply: { body: {}, status: 200 } }] },
  ])("rejects malformed input %j", (given) => {
    expect(() => validateCase({ ...caseWith(), given })).toThrow(/rhino-local: case.given\./);
  });
});

describe("action and ESV builders on Rhino", () => {
  const suite = defineSuite({
    name: "action-esv-channels",
    script: 'var reply = openidm.action("endpoint/example", "evaluate"); action.goTo(reply + ":" + systemEnv.getProperty("esv.missing", "absent"));',
    outcomes: ["test:absent", "hook:absent", "suite:absent"],
    always: { openidmActions: [suiteReply], esvUndeclared: "error" },
    beforeRun: ({ request }) => {
      if (request.state.shared.hook === true) {
        request.openidmActions.unshift({ ...suiteReply, reply: { body: "hook" } });
        request.esvUndeclared = "absent";
      }
    },
  });
  const lease = useLease(suite);

  it("carries per-test replies and ESV policy into a real JVM evaluation", async () => {
    const run = await lease.run().openidmActions([suiteReply]).openidmActions([testReply]).esvUndeclared("absent").expect({
      outcome: "test:absent",
      openidm: [{ method: "action", resource: "endpoint/example", actionName: "evaluate" }],
    });
    expect(run.verdict.pass, run.verdict.summary).toBe(true);
  });

  it("carries beforeRun edits into a real JVM evaluation", async () => {
    const run = await lease.run().state({ shared: { hook: true } }).expect({
      outcome: "hook:absent",
      openidm: [{ method: "action", resource: "endpoint/example", actionName: "evaluate" }],
    });
    expect(run.verdict.pass, run.verdict.summary).toBe(true);
  });
});
