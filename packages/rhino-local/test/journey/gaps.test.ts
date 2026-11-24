/** Journey emulator gaps found by rewriting real journeys; generic trees only. */
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { RhinoRunner } from "../../src/runner.ts";
import { IdmConfig, IdmEmulator } from "../../src/idm/index.ts";
import { JourneyConfig, JourneyEmulator, type JourneyOptions } from "../../src/journey/index.ts";
import { expectEmulationFailure } from "../../src/harness/emulation-guard.ts";
import type { JsonValue } from "../../src/protocol.ts";
import { SUCCESS_NODE_ID as SUCCESS, FAILURE_NODE_ID as FAILURE } from "../../src/aic/constants.ts";

let runner: RhinoRunner;
beforeAll(async () => {
  runner = await RhinoRunner.spawn();
}, 60_000);
afterAll(async () => {
  await runner?.close();
});

const b64 = (source: string) => Buffer.from(source).toString("base64");
const decision = (source: string, name = "invented-script") => ({
  name,
  evaluatorVersion: "2.0",
  language: "JAVASCRIPT",
  context: "AUTHENTICATION_TREE_DECISION_NODE",
  script: b64(source),
});
const library = (name: string, source: string) => ({
  name,
  evaluatorVersion: "2.0",
  language: "JAVASCRIPT",
  context: "LIBRARY",
  script: b64(source),
});

/** One scripted node routing true/false to Success/Failure; extra scripts are the libraries. */
function make(
  source: string,
  scripts: Record<string, ReturnType<typeof library>> = {},
  options: JourneyOptions = {},
) {
  return new JourneyEmulator(
    new JourneyConfig({
      realm: "alpha",
      trees: {
        invented: {
          entryNodeId: "one",
          nodes: {
            one: { nodeType: "ScriptedDecisionNode", connections: { true: SUCCESS, false: FAILURE } },
          },
        },
      },
      nodes: { ScriptedDecisionNode: { one: { script: "main", outcomes: ["true", "false"] } } },
      scripts: { main: decision(source), ...scripts },
    }),
    runner,
    new IdmEmulator(new IdmConfig({ sync: { mappings: [] }, managed: { objects: [] } }), runner),
    options,
  );
}
const start = (emu: JourneyEmulator, body: Record<string, JsonValue> = {}) =>
  emu.request({
    method: "POST",
    path: "authenticate",
    query: { authIndexType: "service", authIndexValue: "invented" },
    body,
  });

describe("G1: require of library scripts and systemEnv", () => {
  const libs = {
    "id-a": library(
      "lib-a",
      `var b = require("lib-b");
       exports.read = function (k) { return systemEnv.getProperty(k); };
       exports.twice = function (n) { return b.double(n); };`,
    ),
    "id-b": library("lib-b", `exports.double = function (n) { return n * 2; };`),
  };
  const body = (check: string) =>
    `var a = require("lib-a");
     nodeState.putShared("username", "u");
     action.goTo(${check} ? "true" : "false");`;

  it("a library read of systemEnv sees the configured ESV, and a nested require resolves", async () => {
    const emu = make(
      body(`a.read("esv.invented.url") === "https://x.example.com" && a.twice(21) === 42`),
      libs,
      { esv: { "esv.invented.url": "https://x.example.com" } },
    );
    expect((await start(emu)).status).toBe(200);
  });

  it("an ESV that is not configured reads as null, not as a failure (discriminates a stub that throws)", async () => {
    const emu = make(body(`a.read("esv.invented.absent") === null`), libs, {});
    expect((await start(emu)).status).toBe(200);
  });

  it("a decision script reads systemEnv directly", async () => {
    const emu = make(
      `nodeState.putShared("username","u");
       action.goTo(systemEnv.getProperty("esv.invented.flag") === "on" ? "true" : "false");`,
      {},
      { esv: { "esv.invented.flag": "on" } },
    );
    expect((await start(emu)).status).toBe(200);
  });

  it("a library the config does not hold is a failure naming it, never a silent undefined", async () => {
    expectEmulationFailure(/require: library "lib-nowhere" is not declared.*/);
    const emu = make(`require("lib-nowhere"); action.goTo("true");`, {});
    const result = await start(emu);
    expect(result.status).not.toBe(200);
    expect(JSON.stringify(result.body)).toContain("lib-nowhere");
  });

  it("a decision script is not a library: require resolves LIBRARY-context scripts only", async () => {
    expectEmulationFailure(/require: library "invented-script" is not declared.*/);
    const emu = make(`require("invented-script"); action.goTo("true");`, {});
    const result = await start(emu);
    expect(result.status).not.toBe(200);
  });
});

describe("G1: ESV placeholders in node configuration", () => {
  const withNode = (
    node: Record<string, unknown>,
    esv?: Record<string, string>,
    type = "RetryLimitDecisionNode",
    wires: Record<string, string> = { Retry: SUCCESS, Reject: FAILURE },
  ) =>
    new JourneyEmulator(
      new JourneyConfig({
        realm: "alpha",
        trees: {
          invented: {
            entryNodeId: "seed",
            nodes: {
              seed: { nodeType: "ScriptedDecisionNode", connections: { true: "limit" } },
              limit: { nodeType: type, connections: wires },
            },
          },
        },
        nodes: {
          ScriptedDecisionNode: { seed: { script: "main", outcomes: ["true"] } },
          [type]: { limit: node },
        },
        scripts: { main: decision(`nodeState.putShared("username","u"); action.goTo("true");`) },
      }),
      runner,
      new IdmEmulator(new IdmConfig({ sync: { mappings: [] }, managed: { objects: [] } }), runner),
      { now: () => 1_700_000_000_000_000_000n, ...(esv ? { esv } : {}) },
    );
  it("a $int wrapper holding a placeholder reads the configured number", async () => {
    // Limit 0 is refused by the node, so a resolved 5 is the only way this reaches Retry.
    const emu = withNode(
      { retryLimit: { $int: "&{esv.invented.limit}" } },
      { "esv.invented.limit": "5" },
    );
    expect((await start(emu)).status).toBe(200);
  });
  it("a wrapped literal reads as itself", async () => {
    expect((await start(withNode({ retryLimit: { $int: "2" } }))).status).toBe(200);
  });
  it("a wrapped signing key resolves to the configured value: same cookie as the literal, another as a different key", async () => {
    const cookieOf = async (node: Record<string, unknown>, esv?: Record<string, string>) =>
      (await start(withNode(node, esv, "SetPersistentCookieNode", { outcome: SUCCESS }))).cookies?.[0]
        ?.value;
    const wrapped = await cookieOf(
      { hmacSigningKey: { $string: "&{esv.invented.key}" } },
      { "esv.invented.key": "key-one" },
    );
    expect(wrapped).toBeDefined();
    expect(wrapped).toBe(await cookieOf({ hmacSigningKey: "key-one" }));
    expect(wrapped).not.toBe(await cookieOf({ hmacSigningKey: "key-two" }));
  });
  it("a placeholder with no configured value is missing, not a default", async () => {
    expectEmulationFailure("ESV placeholder not configured: esv.invented.limit");
    const result = await start(withNode({ retryLimit: { $int: "&{esv.invented.limit}" } }));
    expect(result.status).toBe(501);
  });
});
