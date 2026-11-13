import { describe, expect, it } from "vitest";
import { loadBehaviour, runScript, sandboxFaults } from "./load-behaviour.ts";

describe("OpenIDM action replies (D9)", () => {
  it("returns the first matching reply and records the unchanged call", () => {
    const effects = runScript('var reply = openidm.action("endpoint/example", "evaluate", { input: true }); nodeState.putShared("reply", reply);', {
      openidmActions: [
        { match: { resource: /endpoint\/.*/, action: "other" }, reply: { body: "wrong action" } },
        { match: { resource: "endpoint/other", action: "evaluate" }, reply: { body: "wrong resource" } },
        { match: { resource: /endpoint\/.*/, action: "evaluate" }, reply: { body: { accepted: true } } },
        { match: { resource: "endpoint/example", action: "evaluate" }, reply: { body: false } },
      ],
    });
    expect(effects.sharedState.final.reply).toEqual({ accepted: true });
    expect(effects.openidm).toEqual([{ method: "action", resource: "endpoint/example", actionName: "evaluate", body: { input: true } }]);
  });

  it.each([null, "reply", 42, false, [1]])("returns a JSON reply body %j", (body) => {
    const effects = runScript('nodeState.putShared("reply", openidm.action("endpoint/example", "evaluate"));', {
      openidmActions: [{ match: { resource: "endpoint/example", action: "evaluate" }, reply: { body } }],
    });
    expect(effects.sharedState.final.reply).toEqual(body);
  });

  it("throws naming the reply channel for an unmatched action", () => {
    expect(() => runScript('openidm.action("endpoint/example", "evaluate");'))
      .toThrow("rhino-local: openidm.action: no given.openidmActions stub for evaluate endpoint/example");
  });

  it("failure stubs win over reply stubs and failed calls remain recorded", () => {
    const effects = runScript('try { openidm.action("endpoint/example", "evaluate"); } catch (e) { nodeState.putShared("error", String(e)); }', {
      openidmFailures: [{ match: { method: "action", resource: "endpoint/example", ordinal: 1 }, reply: { code: 503 } }],
      openidmActions: [{ match: { resource: "endpoint/example", action: "evaluate" }, reply: { body: "ignored" } }],
    });
    expect(effects.sharedState.final.error).toMatch(/JavaException:.*503/);
    expect(effects.openidm).toHaveLength(1);
  });

  // Only the three names are measured (the refusal lists them); the `{}` these used to return
  // was not. Since E15b an unstubbed one faults, and a declared reply still answers.
  it.each(["patch", "triggerSyncCheck", "updateLastSync"])("faults an unstubbed collection %s and allows a declared reply", (action) => {
    const sandbox = loadBehaviour();
    const openidm = sandbox.openidm as { action(resource: string, action: string): unknown };
    expect(() => openidm.action("managed/alpha_user", action))
      .toThrow(`rhino-local: openidm.action: ${action} on "managed/alpha_user" is not emulated`);
    expect(sandboxFaults(sandbox).map((fault) => fault.id)).toEqual([`openidm-managed-collection-action/${action}`]);
    const effects = runScript(`nodeState.putShared("reply", openidm.action("managed/alpha_user", "${action}"));`, {
      openidmActions: [{ match: { resource: "managed/alpha_user", action }, reply: { body: true } }],
    });
    expect(effects.sharedState.final.reply).toBe(true);
  });

  it("cannot override a refused managed-collection action", () => {
    expect(() => runScript('openidm.action("managed/alpha_user", "evaluate");', {
      openidmActions: [{ match: { resource: "managed/alpha_user", action: "evaluate" }, reply: { body: {} } }],
    })).toThrow(/Expecting String containing one of/);
  });

  it("clones a reply for each call", () => {
    const effects = runScript('var reply = openidm.action("endpoint/example", "evaluate"); reply.changed = true; nodeState.putShared("reply", openidm.action("endpoint/example", "evaluate"));', {
      openidmActions: [{ match: { resource: "endpoint/example", action: "evaluate" }, reply: { body: {} } }],
    });
    expect(effects.sharedState.final.reply).toEqual({});
  });
});
