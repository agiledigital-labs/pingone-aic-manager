/**
 * Where a library's errors say they are, and what they are. Library bodies
 * are compiled under their `require` id (java/LibraryHost.java), so the
 * reported line must be the body's own line, and a syntax error must stay the
 * catchable `SyntaxError` the runtime's old `eval` path threw.
 */
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { runCase } from "../../src/bindings/index.ts";
import { defineCase } from "../../src/case/index.ts";
import { RhinoRunner } from "../../src/runner.ts";

describe("library error locations", () => {
  let runner: RhinoRunner;
  beforeAll(async () => {
    runner = await RhinoRunner.spawn();
  }, 60_000);
  afterAll(async () => {
    await runner.close();
  });

  const run = (script: string, libraries: Record<string, string>) =>
    runCase(
      runner,
      defineCase({ name: "library errors", script, outcomes: ["done"], given: { libraries }, expect: { outcome: "done" } })
    );

  it("names the library and the body's own line", async () => {
    await expect(run('require("lib-x"); action.goTo("done");', { "lib-x": "throw new Error('boom on line 1');" })).rejects.toThrow(
      /Error: boom on line 1 \(lib-x#1\)/
    );
    await expect(run('require("lib-x"); action.goTo("done");', { "lib-x": "var a = 1;\n\nthrow new Error('boom');" })).rejects.toThrow(
      /\(lib-x#3\)/
    );
  });

  it("reports a library syntax error as a catchable SyntaxError on the body's line", async () => {
    const result = await run(
      'try { require("lib-y"); } catch (e) { nodeState.putShared("caught", e.name + "|" + e.lineNumber); } action.goTo("done");',
      { "lib-y": "var ok = 1;\nvar x = ;" }
    );
    expect(result.effects.sharedState.final.caught).toBe("SyntaxError|2");
  });
});
