import { afterAll, beforeAll, expect, it } from "vitest";
import { expectEmulationFailure } from "../../src/harness/emulation-guard.ts";
import { IdmConfig, IdmEmulator } from "../../src/idm/index.ts";
import { js } from "./fixtures.ts";
import { RhinoRunner } from "../../src/runner.ts";
let runner: RhinoRunner;
beforeAll(async () => { runner = await RhinoRunner.spawn(); }, 60_000);
afterAll(async () => { await runner?.close(); });
it("Astra #5 measured p20 child inherits language, optimization, features and closure", async () => {
  const result = await runner.eval({ engine: "idm", source: `
    var out={}, parentVar=7;
    spawn(function(){var c=org.mozilla.javascript.Context.getCurrentContext();
      out.language=c.getLanguageVersion();out.optimization=c.getOptimizationLevel();
      out.feature13=c.hasFeature(13);out.feature21=c.hasFeature(21);
      out.letValue=eval('let v=42;v');out.openidmType=typeof openidm;out.parentVar=parentVar;
    }).join();out;
  ` });
  expect(result).toMatchObject({ outcome: "ok", value: { language: 200, optimization: 9, feature13: false, feature21: false, letValue: 42, openidmType: "object", parentVar: 7 } });
  expect(result.measured).toContain("spawn child settings and throwing-child join (orchestrator p20, 2026-11-02)");
});
it("Astra #5 measured p20 joined throwing child leaves the parent unaffected", async () => {
  const result = await runner.eval({ engine: "idm", source: "var t=spawn(function(){throw new Error('child-only')});t.join(5000);({alive:t.isAlive(),parent:42})" });
  expect(result).toMatchObject({ outcome: "ok", value: { alive: false, parent: 42 } });
  expect(result.measured).toContain("spawn child settings and throwing-child join (orchestrator p20, 2026-11-02)");
});
it("Astra #6 disabled deadline refuses spawn before any child starts", async () => {
  expectEmulationFailure("spawn with disabled deadline");
  const result = await runner.eval({ engine: "idm", timeoutMs: 0, bindings: { state: { started: false } }, source: "try {spawn(function(){state.started=true}).join()} catch(e) {} 42;" });
  expect(result.outcome).not.toBe("ok");
  expect(result.emulation?.missing).toContain("spawn with disabled deadline");
  expect(result.bindings?.state).toEqual({ started: false });
});
it("Astra #6 retires a JVM if interrupt cannot stop a spawned Java wait", async () => {
  const owned = await RhinoRunner.spawn({ lane: "host" });
  try {
    expectEmulationFailure("spawned thread outlived its job");
    const result = await owned.eval({ engine: "idm", timeoutMs: 2000, source: `
      var ready=new java.util.concurrent.CountDownLatch(1), gate=new java.util.concurrent.Semaphore(0);
      spawn(function(){ready.countDown();gate.acquireUninterruptibly()});ready.await();42;
    ` });
    expect(result.outcome).not.toBe("ok");
    expectEmulationFailure("rhino-local: JVM retired because a spawned thread survived cleanup");
    const next = await owned.eval({ engine: "idm", source: "42" });
    expect(next.outcome).toBe("protocol_error");
    expect(next.error?.message).toContain("JVM retired");
  } finally { await owned.close(); }
});
// Concurrent log order is unspecified: exercise each JVM independently instead
// of asking primary/shadow comparison to impose a deterministic thread schedule.
const lanes = process.env.AIC_SCRIPT_TESTER_JVM === "both" ? ["host", "container"] as const : ["host"] as const;
it.each(lanes)("Astra #7 %s logging retains 16 x 500 distinct entries in four runs", async (lane) => {
  const owned = await RhinoRunner.spawn({ lane });
  try {
    for (let round = 0; round < 4; round++) {
      const result = await owned.eval({ engine: "idm", timeoutMs: 20_000, source: `
        var ready=new java.util.concurrent.CountDownLatch(16),start=new java.util.concurrent.CountDownLatch(1),ts=[];
        for(var i=0;i<16;i++)ts.push(spawn(function(k){ready.countDown();start.await();for(var j=0;j<500;j++)logger.info('thread {} item {}',k,j)},[i]));
        ready.await();start.countDown();for(var i=0;i<ts.length;i++)ts[i].join();42;
      ` });
      expect(result.outcome).toBe("ok");
      expect(result.logs, `round ${round}`).toHaveLength(8000);
      expect(new Set(result.logs?.map((entry) => entry.message)).size, `round ${round}`).toBe(8000);
    }
  } finally { await owned.close(); }
}, 60_000);
it.each(lanes)("Astra #7 %s shared binding maps and growing lists retain concurrent writes", async (lane) => {
  const owned = await RhinoRunner.spawn({ lane });
  try {
    const result = await owned.eval({ engine: "idm", timeoutMs: 20_000, bindings: { state: { map: {}, list: [] } }, source: `
      var ready=new java.util.concurrent.CountDownLatch(16),start=new java.util.concurrent.CountDownLatch(1),ts=[];
      var errors=new java.util.concurrent.atomic.AtomicInteger(0);
      for(var i=0;i<16;i++)ts.push(spawn(function(k){ready.countDown();start.await();try {for(var j=0;j<500;j++){
        var n=k*500+j;state.map['key'+n]={n:n};state.list[n]=n;
      }}catch(e){errors.incrementAndGet()}},[i]));
      ready.await();start.countDown();for(var i=0;i<ts.length;i++)ts[i].join();
      var valid=0;for(var n=0;n<8000;n++)if(state.map['key'+n].n===n&&state.list[n]===n)valid++;
      ({keys:Object.keys(state.map).length,length:state.list.length,valid:valid,errors:errors.get()});
    ` });
    expect(result).toMatchObject({ outcome: "ok", value: { keys: 8000, length: 8000, valid: 8000, errors: 0 } });
  } finally { await owned.close(); }
}, 60_000);
it.each(lanes)("Astra #7 %s snapshots shared log arguments while siblings mutate them", async (lane) => {
  const owned = await RhinoRunner.spawn({ lane });
  try {
    for (let round = 0; round < 4; round++) {
      const result = await owned.eval({ engine: "idm", timeoutMs: 20_000, bindings: { state: { map: {} } }, source: `
        var ready=new java.util.concurrent.CountDownLatch(16),start=new java.util.concurrent.CountDownLatch(1),ts=[];
        var errors=new java.util.concurrent.atomic.AtomicInteger(0);
        for(var i=0;i<16;i++)ts.push(spawn(function(k){ready.countDown();start.await();try{for(var j=0;j<100;j++){
          state.map['key'+k]=j;logger.info('snapshot {}',state.map);delete state.map['key'+k];
        }}catch(e){errors.incrementAndGet()}},[i]));
        ready.await();start.countDown();for(var i=0;i<ts.length;i++)ts[i].join();errors.get();
      ` });
      expect(result).toMatchObject({ outcome: "ok", value: 0 });
      expect(result.logs).toHaveLength(1600);
    }
  } finally { await owned.close(); }
}, 60_000);

it("Astra #5 REST provenance retains the supplied p20 measurement", async () => {
  const emu = new IdmEmulator(new IdmConfig({ managed: { objects: [] }, endpoints: {
    child: js("var t=spawn(function(){throw new Error('child-only')});t.join();({ok:true});"),
  } }), runner);
  const result = await emu.request({ method: "GET", path: "endpoint/child" });
  expect(result.status).toBe(200);
  expect(result.provenance).toContainEqual({ kind: "measured", behavior: "spawn child settings and throwing-child join (orchestrator p20, 2026-11-02)", source: "docs/api/24-idm-script-runtime.md" });
});
