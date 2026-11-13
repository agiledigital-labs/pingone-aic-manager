import { afterAll, beforeAll, expect, it } from "vitest";
import { RhinoRunner } from "../../../src/runner.ts";
import { expectEmulationFailure } from "../../../src/harness/emulation-guard.ts";
import { IdmConfig, IdmEmulator } from "../../../src/idm/index.ts";
let runner: RhinoRunner;
beforeAll(async () => { runner = await RhinoRunner.spawn(); }, 60_000);
afterAll(async () => { await runner?.close(); }, 30_000);
it("cause coverage", async () => {
  const scenario = process.env.R5_CASE;
  if (scenario === "substring") {
    expectEmulationFailure(/require/);
    const emu = new IdmEmulator(new IdmConfig({ sync: { mappings: [] }, managed: { objects: [{ name: "w", schema: { properties: {} } }] } }), runner);
    await emu.request({ method: "GET", path: "managed/w", query: { _queryId: "prefix-require-suffix" } });
    return;
  }
  if (scenario === "tenant") {
    expect(await runner.eval({ engine: "idm", source: "try{throw {code:501}}catch(e){e.code}" })).toMatchObject({ outcome: "ok", value: 501 });
    return;
  }
  expectEmulationFailure(scenario === "broad" ? /require|spawned thread outlived its job/ : "require");
  if (scenario === "all") expectEmulationFailure("spawned thread outlived its job");
  const result = await runner.eval({ engine: "idm", timeoutMs: 2000, source: `
    try{require('unsupported')}finally{
      var ready=new java.util.concurrent.CountDownLatch(1);
      spawn(function(){ready.countDown();while(true){}});ready.await();
    }` });
  expect(result.missing).toEqual(expect.arrayContaining(["require", "spawned thread outlived its job"]));
});
