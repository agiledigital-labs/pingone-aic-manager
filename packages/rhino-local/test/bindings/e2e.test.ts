import { join } from "node:path";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { runCase } from "../../src/bindings/index.ts";
import { judge } from "../../src/case/index.ts";
import { casesDir } from "../../src/paths.ts";
import { RhinoRunner } from "../../src/runner.ts";
import { decideFromState, openidmRead, writeState } from "../../cases/index.ts";
import { runScript } from "./load-behaviour.ts";

describe("end-to-end cases through the JVM runner", () => {
  let runner: RhinoRunner;

  beforeAll(async () => {
    runner = await RhinoRunner.spawn();
  }, 60_000);

  afterAll(async () => {
    await runner.close();
  }, 30_000);

  it("decide-from-state: read state → goTo", async () => {
    const result = await runCase(runner, decideFromState, {
      sourceName: join(casesDir, "decide-from-state.cjs"),
      timeoutMs: 5_000,
    });
    expect(result.verdict.summary, result.verdict.summary).toBe("");
    expect(result.verdict.pass).toBe(true);
    expect(result.effects.outcome).toBe("true");
  });

  it("write-state: putShared / putTransient", async () => {
    const result = await runCase(runner, writeState, {
      sourceName: join(casesDir, "write-state.cjs"),
      timeoutMs: 5_000,
    });
    expect(result.verdict.summary, result.verdict.summary).toBe("");
    expect(result.verdict.pass).toBe(true);
  });

  it("openidm-read: seeded managed record → state + log", async () => {
    const result = await runCase(runner, openidmRead, {
      sourceName: join(casesDir, "openidm-read.cjs"),
      timeoutMs: 5_000,
    });
    expect(result.verdict.summary, result.verdict.summary).toBe("");
    expect(result.verdict.pass).toBe(true);
    expect(result.effects.openidm).toEqual([
      { method: "read", resource: "managed/alpha_user/alice" },
    ]);
  });

  it("fails the verdict when the script picks the other outcome", async () => {
    const result = await runCase(
      runner,
      {
        ...decideFromState,
        given: { sharedState: { username: "bob" } },
      },
      { timeoutMs: 5_000 }
    );
    expect(result.verdict.pass).toBe(false);
    expect(result.effects.outcome).toBe("false");
    expect(result.verdict.mismatches[0]?.channel).toBe("outcome");
  });
});

describe("vm harvest agrees with judge for the same cases", () => {
  it("passes decide-from-state locally", () => {
    const effects = runScript(decideFromState.script, decideFromState.given);
    expect(judge(decideFromState, effects).pass).toBe(true);
  });

  it("passes write-state locally", () => {
    const effects = runScript(writeState.script, writeState.given);
    expect(judge(writeState, effects).pass).toBe(true);
  });

  it("passes openidm-read locally", () => {
    const effects = runScript(openidmRead.script, openidmRead.given);
    expect(judge(openidmRead, effects).pass).toBe(true);
  });
});
