import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { runCase } from "../../src/bindings/index.ts";
import { defineCase } from "../../src/case/index.ts";
import { RhinoRunner } from "../../src/runner.ts";

/**
 * The unit tests for this binding run in Node's `vm`, which is ES2024 and will
 * accept constructs Rhino does not. The mock leans on `Object.create(null)` and
 * a non-enumerable `defineProperty`, so it has to be exercised on the engine
 * that actually runs author scripts — otherwise the mock's own foundations are
 * only verified against the wrong runtime.
 */
describe("existingSession on the real Rhino engine", () => {
  let runner: RhinoRunner;

  beforeAll(async () => {
    runner = await RhinoRunner.spawn();
  }, 60_000);

  afterAll(async () => {
    await runner.close();
  }, 30_000);

  it("exposes the measured surface to a script under the class shutter", async () => {
    const result = await runCase(
      runner,
      defineCase({
        name: "existing-session-surface",
        script: [
          "var probe = {",
          "  t: typeof existingSession,",
          "  dot: String(existingSession.UserId),",
          '  get: String(existingSession.get("UserId")),',
          '  missing: String(existingSession.get("nope")),',
          "  size: existingSession.size(),",
          "  hasOwn: typeof existingSession.hasOwnProperty,",
          "  str: String(existingSession)",
          "};",
          "var names = [];",
          "for (var k in existingSession) { names.push(k); }",
          "probe.names = names.sort().join(',');",
          'try { existingSession.keySet(); probe.keySet = "returned"; }',
          "catch (e) { probe.keySet = String(e).indexOf('class shutter') >= 0 ? 'threw' : String(e); }",
          "logger.info(JSON.stringify(probe));",
          'action.goTo("true");',
        ].join("\n"),
        outcomes: ["true"],
        given: { existingSession: { UserId: "alice", rlProbe: "hello" } },
        expect: { outcome: "true" },
      }),
      { timeoutMs: 5_000 }
    );
    expect(result.verdict.summary).toBe("");
    const probe = JSON.parse(result.effects.logs[0]?.message ?? "{}") as Record<string, unknown>;
    expect(probe).toEqual({
      t: "object",
      dot: "alice",
      get: "alice",
      missing: "null",
      size: 2,
      hasOwn: "undefined",
      names: "UserId,rlProbe",
      keySet: "threw",
      str: '{ "UserId": "alice", "rlProbe": "hello" }',
    });
  });

  it("leaves the binding undefined when no session is seeded", async () => {
    const result = await runCase(
      runner,
      defineCase({
        name: "existing-session-absent",
        script: 'logger.info(typeof existingSession); action.goTo("true");',
        outcomes: ["true"],
        given: {},
        expect: { outcome: "true", logs: [{ level: "info", message: "undefined" }] },
      }),
      { timeoutMs: 5_000 }
    );
    expect(result.verdict.summary).toBe("");
  });
});
