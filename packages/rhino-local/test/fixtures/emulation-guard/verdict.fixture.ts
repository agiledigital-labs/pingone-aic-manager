/** Inner suite is expected to fail: outer regression checks the actual hooks. */
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { RhinoRunner } from "../../../src/runner.ts";
import { IdmConfig, IdmEmulator } from "../../../src/idm/index.ts";
import * as harness from "../../../src/harness/vitest.ts";
import { defineSuite } from "../../../src/harness/lease.ts";

let runner: RhinoRunner;
let emu: IdmEmulator;
beforeAll(async () => {
  runner = await RhinoRunner.spawn();
  emu = new IdmEmulator(new IdmConfig({ managed: { objects: [] }, endpoints: {
    probe: { type: "text/javascript", source: "require('unsupported');" },
    tenant: { type: "text/javascript", source: "throw {code:501,message:'tenant error'};" },
  } }), runner);
}, 60_000);
afterAll(async () => { await runner?.close(); }, 30_000);

// Without the new helper these bodies pass. The outer test must detect that
// false pass on the unfixed implementation, rather than an import failure.
const install = (harness as typeof harness & { useEmulationGuard?:
  (...sources: Array<() => RhinoRunner | IdmEmulator | undefined>) => void }).useEmulationGuard;
describe("REST guard", () => {
  install?.(() => emu);
  it("generic REST status assertion passes but guard must fail", async () => {
    const result = await emu.request({ method: "GET", path: "endpoint/probe" });
    expect(result.status).toBeGreaterThanOrEqual(400);
  });
  it("tenant 501 satisfies the assertion and guard must pass", async () => {
    const result = await emu.request({ method: "GET", path: "endpoint/tenant" });
    expect(result.status).toBe(501);
    expect(result.emulation).toBeUndefined();
  });
});

describe("runner guard", () => {
  install?.(() => runner);
  it("generic runner error assertion passes but guard must fail", async () => {
    const result = await runner.eval({ engine: "idm", source: "require('unsupported');" });
    expect(result.outcome).not.toBe("ok");
  });
});

describe("automatic useLease protection", () => {
  const lease = harness.useLease(defineSuite({ name: "guard timeout", script: "while(true){}", outcomes: ["done"] }), { timeoutMs: 80 });
  it("generic lease rejection assertion passes but automatic guard must fail", async () => {
    await expect(lease.run().expect({ outcome: "done" })).rejects.toThrow();
  });
});
