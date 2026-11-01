import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { RhinoRunner } from "../../../src/runner.ts";
import { EmulationFailure } from "../../../src/emulation.ts";
import { IdmConfig, IdmEmulator } from "../../../src/idm/index.ts";
import * as harness from "../../../src/harness/vitest.ts";

let runner: RhinoRunner;
const config = new IdmConfig({ managed: { objects: [{ name: "w", schema: { properties: {} } }] }, endpoints: {
  tenant: { type: "text/javascript", source: "throw {code:501,message:'tenant error'};" },
} });
const make = () => new IdmEmulator(config, runner);
const missing = (emu: IdmEmulator, name = "missing") => emu.request({ method: "GET", path: "managed/w", query: { _queryId: name } });
const clean = (emu: IdmEmulator) => emu.request({ method: "GET", path: "managed/w", query: { _queryFilter: "true" } });
const claim = (harness as typeof harness & { expectEmulationFailure?: (name: string | RegExp) => void }).expectEmulationFailure;
beforeAll(async () => { runner = await RhinoRunner.spawn(); }, 60_000);
afterAll(async () => { await runner?.close(); }, 30_000);

describe("detached", () => {
  harness.useEmulationGuard(() => runner);
  it("detached child", async () => {
    await runner.eval({ engine: "idm", timeoutMs: 2000, source: `var ready=new java.util.concurrent.CountDownLatch(1);
      spawn(function(){ready.countDown();while(true){}});ready.await();42;` });
    await new Promise((resolve) => setTimeout(resolve, 300));
  });
});

describe("outstanding eval", () => {
  harness.useEmulationGuard(() => runner);
  let release!: () => void;
  let pending: ReturnType<RhinoRunner["eval"]>;
  afterAll(async () => { release(); await pending; });
  it("unawaited eval", async () => {
    let entered!: () => void;
    const started = new Promise<void>((resolve) => { entered = resolve; });
    const gate = new Promise<void>((resolve) => { release = resolve; });
    pending = runner.eval({ engine: "idm", source: "openidm.read('wait');" }, { onCall: async () => {
      entered(); await gate;
      throw new EmulationFailure({ kind: "missing", message: "late owned failure", missing: ["late owned failure"] });
    } });
    await started;
  });
  it("clean after late eval", async () => {
    release(); await pending;
    expect(await runner.eval({ engine: "idm", source: "42" })).toMatchObject({ outcome: "ok", value: 42 });
  });
});

describe("late callback", () => {
  let release!: () => void;
  let completed!: () => void;
  const callbackDone = new Promise<void>((resolve) => { completed = resolve; });
  afterAll(async () => { release(); await callbackDone; });
  it("unfinished callback after timeout", async () => {
    claim?.("IDM harness timeout");
    const gate = new Promise<void>((resolve) => { release = resolve; });
    await runner.eval({ engine: "idm", timeoutMs: 80, source: "openidm.read('wait');" }, { onCall: async () => {
      try {
        await gate;
        throw new EmulationFailure({ kind: "missing", message: "late callback failure", missing: ["late callback failure"] });
      } finally { completed(); }
    } });
  });
  it("clean after late callback", async () => {
    release(); await callbackDone;
    expect(await runner.eval({ engine: "idm", source: "42" })).toMatchObject({ outcome: "ok", value: 42 });
    expect(runner.emulationFailures.find((entry) => entry.failure.message === "late callback failure")?.owner?.name)
      .toBe("unfinished callback after timeout");
  });
});

describe("journal reset", () => {
  let emu: IdmEmulator;
  beforeAll(() => { emu = make(); });
  harness.useEmulationGuard(() => emu);
  it("reset cannot erase", async () => {
    const child = emu.fork();
    await missing(child);
    (child as IdmEmulator & { resetEmulationFailures?: () => void }).resetEmulationFailures?.();
  });
});

for (const replacement of ["replace", "clear"] as const) describe(replacement, () => {
  let emu: IdmEmulator | undefined;
  beforeAll(() => { emu = make(); });
  harness.useEmulationGuard(() => emu);
  it(`${replacement} fixture`, async () => {
    await missing(emu!);
    emu = replacement === "replace" ? make() : undefined;
  });
});

let parent: IdmEmulator;
let a: IdmEmulator;
let b: IdmEmulator;
beforeAll(() => { parent = make(); a = parent.fork(); b = parent.fork(); });
describe("sequential A", () => {
  harness.useEmulationGuard(() => a);
  it("sequential failing sibling", async () => { await missing(a); });
});
describe("sequential B", () => {
  harness.useEmulationGuard(() => b);
  it("sequential clean sibling", async () => { expect((await clean(b)).status).toBe(200); });
});

describe("concurrent siblings", () => {
  let failed!: () => void;
  const failureRecorded = new Promise<void>((resolve) => { failed = resolve; });
  harness.useEmulationGuard(() => a, () => b);
  it.concurrent("concurrent failing sibling", async () => { await missing(a, "concurrent missing"); failed(); });
  it.concurrent("concurrent clean sibling", async () => { await failureRecorded; expect((await clean(b)).status).toBe(200); });
});

describe("expected failures", () => {
  let emu: IdmEmulator;
  beforeAll(() => { emu = make(); });
  harness.useEmulationGuard(() => emu);
  it("named expected failure retained", async () => {
    claim?.("named _queryId: intended");
    await missing(emu, "intended");
    expect(emu.emulationFailures).toHaveLength(1);
    expect(emu.emulationFailures[0]?.expected).toBe("named _queryId: intended");
  });
  it("absent expected failure", () => { claim?.("never occurred"); });
});

it("unnamed source", async () => { await missing(make()); });

describe("outstanding request", () => {
  let emu: IdmEmulator;
  let release!: () => void;
  let pending: ReturnType<IdmEmulator["request"]>;
  harness.useEmulationGuard(() => emu);
  afterAll(async () => { release(); await pending; });
  it("unawaited request", async () => {
    let entered!: () => void;
    const started = new Promise<void>((resolve) => { entered = resolve; });
    const gate = new Promise<void>((resolve) => { release = resolve; });
    emu = new IdmEmulator(config, runner, { policy: async () => {
      entered(); await gate;
      throw new EmulationFailure({ kind: "missing", message: "late REST failure", missing: ["late REST failure"] });
    } });
    pending = emu.request({ method: "PUT", path: "managed/w/one", body: {} });
    await started;
  });
});

it("ordinary tenant 501", async () => { expect((await make().request({ method: "GET", path: "endpoint/tenant" })).status).toBe(501); });

it("duplicate module registry", async () => {
  claim?.("named _queryId: duplicate graph");
  await missing(make(), "duplicate graph");
  const url = new URL("../../../src/emulation.ts?ownership-copy", import.meta.url);
  const duplicate = await import(url.href) as typeof import("../../../src/emulation.ts");
  expect(duplicate.recordedEmulationFailures().find((entry) => entry.failure.missing?.includes("named _queryId: duplicate graph"))?.owner?.name)
    .toBe("duplicate module registry");
});
