import { afterAll, expect, it } from "vitest";
import { RhinoRunner } from "../../../src/runner.ts";
import { IdmConfig, IdmEmulator } from "../../../src/idm/index.ts";
const runner = await RhinoRunner.spawn();
afterAll(() => runner.close());
const config = new IdmConfig({ sync: { mappings: [] }, managed: { objects: [{ name: "w", schema: { properties: {} } }] } });
const emu = new IdmEmulator(config, runner);
const scenario = process.env.R5_CASE;
const missing = () => emu.request({ method: "GET", path: "managed/w", query: { _queryId: `${scenario} missing` } });
if (scenario === "import") {
  await missing();
  it("clean after unowned import", () => { expect(emu.emulationFailures[0]?.owner).toBeUndefined(); });
} else if (scenario === "unowned-outstanding") {
  const blocked = new IdmEmulator(config, runner, { policy: () => new Promise(() => {}) });
  void blocked.request({ method: "PUT", path: "managed/w/blocked", body: {} });
  it("clean after unowned outstanding", () => {});
} else if (scenario === "fails") {
  it.fails("expected-fail cannot invert emulation", async () => { await missing(); });
} else if (scenario === "retry") {
  let attempt = 0;
  it("retry cannot discard emulation", { retry: 1 }, async () => {
    if (++attempt === 1) await missing();
    else expect((await emu.request({ method: "GET", path: "managed/w", query: { _queryFilter: "true" } })).status).toBe(200);
  });
} else if (scenario === "skip") {
  it("skip cannot suppress emulation", async (context) => { await missing(); context.skip(); });
}
