/**
 * A required library keeps the common bindings from when the preamble
 * finished (docs/api/12, "Rebinding logger around require()", measured
 * 2026-11-14). A caller variable named logger does not replace the library's
 * logger. nodeState stays the engine's ReferenceError. Driven through the
 * public case API.
 */
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { runCase } from "../../src/bindings/run.ts";
import { defineCase } from "../../src/case/index.ts";
import { RhinoRunner } from "../../src/runner.ts";

const LIB = [
  "exports.load = function () {",
  "  return { info: function (m) { logger.info(m); } };",
  "};",
  "exports.name = function () { return logger.getName(); };",
  "exports.readCallerOnly = function () { return String(nodeState); };",
].join("\n");

const SCRIPT_ID = "script-id-1";
const DECISION_NAME = "widget-decision";
const DECISION_LOGGER =
  `scripts.AUTHENTICATION_TREE_DECISION_NODE.${SCRIPT_ID}.(${DECISION_NAME})`;
const MAY_ACT_NAME = "widget-may-act";
const MAY_ACT_LOGGER =
  `scripts.OAUTH2_MAY_ACT_NEXT_GEN.${SCRIPT_ID}.(${MAY_ACT_NAME})`;

describe("a required library keeps its own logger", () => {
  let runner: RhinoRunner;
  beforeAll(async () => {
    runner = await RhinoRunner.spawn();
  }, 60_000);
  afterAll(async () => {
    await runner.close();
  });

  const libraries = { logging: LIB };

  it("logs once when the caller rebinds logger, under the caller's script name", async () => {
    const { effects, verdict } = await runCase(runner, defineCase({
      name: "rebound logger",
      script: [
        'var logger = require("logging").load();',
        'logger.info("shadow");',
        'nodeState.putShared("loggerName", require("logging").name());',
        'action.goTo("done");',
      ].join("\n"),
      outcomes: ["done"],
      given: { libraries, scriptName: DECISION_NAME, loggerScriptId: SCRIPT_ID },
      expect: {
        outcome: "done",
        logs: [{ level: "info", message: "shadow" }],
        sharedState: { added: { loggerName: DECISION_LOGGER } },
      },
    }));
    expect(verdict.pass, verdict.summary).toBe(true);
    expect(effects.logs).toEqual([{ level: "info", message: "shadow" }]);
  }, 60_000);

  it("logs once when the caller names the binding log", async () => {
    const { effects, verdict } = await runCase(runner, defineCase({
      name: "logger control",
      script: [
        'var log = require("logging").load();',
        'log.info("plain");',
        'nodeState.putShared("loggerName", require("logging").name());',
        'action.goTo("done");',
      ].join("\n"),
      outcomes: ["done"],
      given: { libraries, scriptName: DECISION_NAME, loggerScriptId: SCRIPT_ID },
      expect: {
        outcome: "done",
        logs: [{ level: "info", message: "plain" }],
        sharedState: { added: { loggerName: DECISION_LOGGER } },
      },
    }));
    expect(verdict.pass, verdict.summary).toBe(true);
    expect(effects.logs).toEqual([{ level: "info", message: "plain" }]);
  }, 60_000);

  it("keeps nodeState unbound inside the library", async () => {
    const { effects, verdict } = await runCase(runner, defineCase({
      name: "library nodeState",
      script: [
        "var caught = \"\";",
        "var name = \"\";",
        "try { require(\"logging\").readCallerOnly(); } catch (e) {",
        "  caught = String(e);",
        "  name = e.name;",
        "}",
        "nodeState.putShared(\"error\", caught);",
        "nodeState.putShared(\"name\", name);",
        "action.goTo(\"done\");",
      ].join("\n"),
      outcomes: ["done"],
      given: { libraries, scriptName: DECISION_NAME, loggerScriptId: SCRIPT_ID },
      expect: {
        outcome: "done",
        sharedState: {
          added: {
            error: /^ReferenceError: "nodeState" is not defined\./,
            name: "ReferenceError",
          },
        },
      },
    }));
    expect(verdict.pass, verdict.summary).toBe(true);
    expect(effects.sharedState.final.error).toMatch(/^ReferenceError: "nodeState" is not defined\./);
    expect(effects.sharedState.final.name).toBe("ReferenceError");
  }, 60_000);

  it("logs once from OAUTH2_MAY_ACT when the caller rebinds logger", async () => {
    const { effects, verdict } = await runCase(runner, defineCase({
      name: "may-act rebound logger",
      context: "OAUTH2_MAY_ACT",
      script: [
        'var logger = require("logging").load();',
        'logger.info("shadow");',
        'token.setMayAct({ name: require("logging").name() });',
      ].join("\n"),
      given: { libraries, scriptName: MAY_ACT_NAME, loggerScriptId: SCRIPT_ID },
      expect: {
        logs: [{ level: "info", message: "shadow" }],
        mayAct: { mayAct: { name: MAY_ACT_LOGGER } },
      },
    }));
    expect(verdict.pass, verdict.summary).toBe(true);
    expect(effects.logs).toEqual([{ level: "info", message: "shadow" }]);
    expect(effects.mayAct?.mayAct).toEqual({ name: MAY_ACT_LOGGER });
  }, 60_000);
});
