/**
 * Case isolation under compiled-script caching and JVM reuse.
 *
 * Before the cache, every case compiled and ran the whole preamble into a
 * fresh scope, so nothing one script did could reach the next. The runner now
 * shares compiled code across cases (java/ScriptCache.java) and `useLease`
 * borrows JVMs that outlive a test file (src/pool.ts). These tests pin that
 * a case still starts exactly as if fresh: after one that vandalised every
 * kind of state a script can reach, in the same runner and in a later
 * "file" (a release and re-borrow from the pool, which is the path the next
 * file in a worker takes), and after a case that timed out, crashed or retired
 * the JVM.
 */
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { runCase } from "../../src/bindings/index.ts";
import { defineCase } from "../../src/case/index.ts";
import type { Given } from "../../src/case/types.ts";
import { expectEmulationFailure } from "../../src/harness/emulation-guard.ts";
import { JvmPool } from "../../src/pool.ts";
import { RhinoRunner } from "../../src/runner.ts";

const LIBRARIES = {
  // Module state, a regex literal, and a function that reads a global through
  // the library's own scope chain.
  stateful: [
    "var counter = 0;",
    "var re = /a/g;",
    "exports.bump = function () { counter += 1; return counter; };",
    "exports.re = re;",
    "exports.arrayLeak = function () { return [].leakArray; };",
  ].join("\n"),
  // A tagged template: Rhino caches its call-site array inside the compiled script.
  tagged: "function tag(strings) { return strings; }\nexports.site = function () { return tag`a${1}b`; };",
};

/** Everything a script can reach, vandalised. Non-enumerable so the case's own harvest survives. */
const POLLUTER = `
leakGlobal = 1;
Object.defineProperty(Array.prototype, "leakArray", { value: 1, configurable: true });
Object.defineProperty(Object.prototype, "leakObject", { value: 1, configurable: true });
String.prototype.trim = function () { return "poisoned"; };
var lib = require("stateful");
lib.bump();
lib.bump();
lib.re.lastIndex = 7;
lib.re.leak = 1;
require("tagged").site();
nodeState.leak = 1;
callbacksBuilder.leak = 1;
utils.crypto.randomUUID = function () { return "poisoned"; };
logger.error("polluter log");
nodeState.putShared("polluted", true);
JSON.leak = 1;
// Replaced, not broken: the case's own harvest still has to parse.
JSON.parse = (function (parse) { var f = function () { return parse.apply(JSON, arguments); }; f.poisoned = true; return f; })(JSON.parse);
action.goTo("done");
`;

const PROBE = `
var lib = require("stateful");
var probe = {
  leakGlobal: typeof leakGlobal,
  arrayProto: [].leakArray === undefined,
  objectProto: ({}).leakObject === undefined,
  trim: " a ".trim(),
  json: JSON.leak === undefined && JSON.parse.poisoned === undefined,
  nodeState: nodeState.leak === undefined,
  callbacksBuilder: callbacksBuilder.leak === undefined,
  uuid: utils.crypto.randomUUID() !== "poisoned",
  moduleState: lib.bump(),
  regexLastIndex: lib.re.lastIndex,
  regexProps: lib.re.leak === undefined,
  libraryGlobals: lib.arrayLeak() === undefined,
  taggedSite: require("tagged").site().leakArray === undefined,
};
nodeState.putShared("probe", JSON.stringify(probe));
action.goTo("done");
`;

const FRESH = {
  leakGlobal: "undefined",
  arrayProto: true,
  objectProto: true,
  trim: "a",
  json: true,
  nodeState: true,
  callbacksBuilder: true,
  uuid: true,
  moduleState: 1,
  regexLastIndex: 0,
  regexProps: true,
  libraryGlobals: true,
  taggedSite: true,
};

async function runScript(runner: RhinoRunner, script: string, given: Given = { libraries: LIBRARIES }) {
  const kase = defineCase({ name: "isolation", script, outcomes: ["done"], given, expect: { outcome: "done" } });
  return (await runCase(runner, kase)).effects;
}

async function pollute(runner: RhinoRunner): Promise<void> {
  const effects = await runScript(runner, POLLUTER);
  // The polluter really ran, to the end, in its own case.
  expect(effects.outcome).toBe("done");
  expect(effects.sharedState.final.polluted).toBe(true);
  expect(effects.logs.map((log) => log.message)).toContain("polluter log");
}

async function probe(runner: RhinoRunner): Promise<void> {
  const effects = await runScript(runner, PROBE);
  expect(JSON.parse(String(effects.sharedState.final.probe))).toEqual(FRESH);
  expect(effects.sharedState.final.polluted).toBeUndefined();
  expect(effects.logs.map((log) => log.message)).not.toContain("polluter log");
}

async function jvmPid(runner: RhinoRunner): Promise<number> {
  const response = await runner.eval({ source: "Number(java.lang.ProcessHandle.current().pid())" });
  expect(response.outcome).toBe("ok");
  return response.value as number;
}

describe("case isolation in one runner", () => {
  let runner: RhinoRunner;
  beforeAll(async () => {
    runner = await RhinoRunner.spawn();
  }, 60_000);
  afterAll(async () => {
    await runner.close();
  });

  it("a probe in a fresh JVM sees fresh state (control)", async () => {
    await probe(runner);
  });

  it("the next case after a polluter starts exactly as if fresh", async () => {
    await pollute(runner);
    await probe(runner);
    // And again: the second probe is served entirely from cached compiled scripts.
    await pollute(runner);
    await probe(runner);
  });

  it("concurrent cases do not see each other", async () => {
    await Promise.all([pollute(runner), probe(runner), pollute(runner), probe(runner)]);
  });

  it("a library edited between runs recompiles", async () => {
    const value = async (body: string): Promise<unknown> => {
      const effects = await runScript(
        runner,
        'nodeState.putShared("v", require("edited").v); action.goTo("done");',
        { libraries: { edited: body } }
      );
      return effects.sharedState.final.v;
    };
    expect(await value("exports.v = 1;")).toBe(1);
    expect(await value("exports.v = 2;")).toBe(2);
    expect(await value("exports.v = 1;")).toBe(1);
  });
});

describe("case isolation across files on a pooled JVM", () => {
  let pool: JvmPool;
  beforeAll(async () => {
    pool = await JvmPool.start();
  });
  afterAll(async () => {
    await pool.stop();
  }, 30_000);

  /** One "file": borrow the slot's JVM, use it, give it back. */
  async function file<T>(fn: (runner: RhinoRunner) => Promise<T>): Promise<T> {
    const runner = await RhinoRunner.borrow(pool.address, { lane: "host" });
    try {
      return await fn(runner);
    } finally {
      await runner.close();
    }
  }

  it("the next file reuses the JVM and starts as if fresh", async () => {
    const first = await file(async (runner) => {
      await pollute(runner);
      return jvmPid(runner);
    });
    const second = await file(async (runner) => {
      await probe(runner);
      return jvmPid(runner);
    });
    expect(second).toBe(first);
  });

  it("a timed-out case does not poison the JVM for the next file", async () => {
    const first = await file(async (runner) => {
      expectEmulationFailure("Rhino harness timeout");
      const response = await runner.eval({ source: "for (;;) {}", timeoutMs: 200 });
      expect(response.outcome).toBe("timeout");
      return jvmPid(runner);
    });
    const second = await file(async (runner) => {
      await probe(runner);
      return jvmPid(runner);
    });
    expect(second).toBe(first);
  });

  it("a JVM that crashed is replaced, not lent again", async () => {
    const crashed = await file(async (runner) => {
      const pid = await jvmPid(runner);
      expectEmulationFailure(/RhinoRunnerExitError: rhino-local: JVM runner exited[\s\S]*/);
      process.kill(pid, "SIGKILL");
      await expect(runner.eval({ source: "1" })).rejects.toThrow(/exited/);
      return pid;
    });
    const next = await file(async (runner) => {
      await probe(runner);
      return jvmPid(runner);
    });
    expect(next).not.toBe(crashed);
  });

  it("a JVM that retired itself is replaced, not lent again", async () => {
    const retired = await file(async (runner) => {
      const pid = await jvmPid(runner);
      expectEmulationFailure("spawned thread outlived its job");
      const result = await runner.eval({
        engine: "idm",
        timeoutMs: 2000,
        source: `
          var ready=new java.util.concurrent.CountDownLatch(1), gate=new java.util.concurrent.Semaphore(0);
          spawn(function(){ready.countDown();gate.acquireUninterruptibly()});ready.await();42;
        `,
      });
      expect(result.outcome).not.toBe("ok");
      return pid;
    });
    const next = await file(async (runner) => {
      await probe(runner);
      return jvmPid(runner);
    });
    expect(next).not.toBe(retired);
  });
});
