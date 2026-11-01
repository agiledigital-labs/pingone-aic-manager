import { afterAll, beforeAll, expect, it } from "vitest";
import { RhinoRunner } from "../../src/runner.ts";

import { expectEmulationFailure } from "../../src/harness/vitest.ts";

let runner: RhinoRunner;
beforeAll(async () => { runner = await RhinoRunner.spawn(); }, 60_000);
afterAll(async () => { await runner?.close(); }, 30_000);

it.each([
  ["observer", "while(true){}"],
  ["delayed missing", "java.lang.Thread.sleep(1000);require('unsupported');"],
])("R4 #1 detached %s thread fails before publishing a job result", async (_name, body) => {
  const result = await runner.eval({ engine: "idm", timeoutMs: 2000, source: `
    var ready=new java.util.concurrent.CountDownLatch(1);
    spawn(function(){ready.countDown();${body}});
    ready.await();42;
  ` });
  expect(result.outcome).not.toBe("ok");
  expect(result.missing).toContain("spawned thread outlived its job");
  expectEmulationFailure("spawned thread outlived its job");
  expect(runner.emulationFailures).toEqual(expect.arrayContaining([
    expect.objectContaining({ failure: expect.objectContaining({ kind: "missing" }) }),
  ]));
});

it("R4 #1 a joined completed child preserves shell spawn arguments and success", async () => {
  const result = await runner.eval({ engine: "idm", timeoutMs: 0, source: `
    var out=new java.util.concurrent.atomic.AtomicInteger(0);
    spawn(function(a,b){out.set(a+b)},[19,23]).join();out.get();
  ` });
  expect(result).toMatchObject({ outcome: "ok", value: 42 });
});

it("R4 #1 sticky earlier failure retains the detached-thread boundary too", async () => {
  expectEmulationFailure("require");
  expectEmulationFailure("spawned thread outlived its job");
  const result = await runner.eval({ engine: "idm", timeoutMs: 2000, source: `
    try { require('unsupported'); } finally {
      var ready=new java.util.concurrent.CountDownLatch(1);
      spawn(function(){ready.countDown();while(true){}});
      ready.await();
    }
  ` });
  expect(result.outcome).not.toBe("ok");
  expect(result.missing).toEqual(expect.arrayContaining(["require", "spawned thread outlived its job"]));
  expect(result.emulation?.missing).toEqual(expect.arrayContaining(["require", "spawned thread outlived its job"]));
});
