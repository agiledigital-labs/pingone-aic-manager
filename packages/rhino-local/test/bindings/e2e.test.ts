import { join } from "node:path";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { runCase } from "../../src/bindings/index.ts";
import { defineCase, judge } from "../../src/case/index.ts";
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

  // The JVM path recognises the legacy classes by their Rhino package name,
  // not by identity, so it needs its own positive and negative controls.
  it("scopes Action and HiddenValueCallback to the importer that names them", async () => {
    const t = (key: string, expr: string): string =>
      `nodeState.putShared(${JSON.stringify(key)}, typeof ${expr});`;
    const scoped = defineCase({
      name: "importer-scope",
      given: { engine: "legacy", callbacks: [] },
      outcomes: ["done"],
      script: [
        t("emptyAction", "JavaImporter().Action"),
        t("emptyCallback", "JavaImporter().HiddenValueCallback"),
        t("unrelatedAction", "JavaImporter(java.util).Action"),
        t("unrelatedCallback", "JavaImporter(java.util).HiddenValueCallback"),
        t("classAction", "JavaImporter(org.forgerock.openam.auth.node.api.Action).Action"),
        t("classActionCallback", "JavaImporter(org.forgerock.openam.auth.node.api.Action).HiddenValueCallback"),
        t("packageAction", "JavaImporter(org.forgerock.openam.auth.node.api).Action"),
        t("classCallback", "JavaImporter(com.sun.identity.authentication.callbacks.HiddenValueCallback).HiddenValueCallback"),
        t("packageCallback", "JavaImporter(com.sun.identity.authentication.callbacks).HiddenValueCallback"),
        'action = JavaImporter(org.forgerock.openam.auth.node.api.Action).Action.goTo("done").build();',
      ].join("\n"),
      expect: {
        outcome: "done",
        sharedState: {
          added: {
            emptyAction: "undefined",
            emptyCallback: "undefined",
            unrelatedAction: "undefined",
            unrelatedCallback: "undefined",
            classAction: "object",
            classActionCallback: "undefined",
            packageAction: "object",
            classCallback: "function",
            packageCallback: "function",
          },
        },
      },
    });
    const result = await runCase(runner, scoped, { timeoutMs: 5_000 });
    expect(result.verdict.summary, result.verdict.summary).toBe("");
    expect(result.verdict.pass).toBe(true);
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
