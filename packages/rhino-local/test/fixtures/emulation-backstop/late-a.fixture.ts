import { it } from "vitest";
import { RhinoRunner } from "../../../src/runner.ts";
import { IdmConfig, IdmEmulator } from "../../../src/idm/index.ts";
export interface LateState { release: () => void; done: Promise<void>; runner: RhinoRunner }
const shared = process as typeof process & { rhinoBackstopLate?: LateState };
const runner = await RhinoRunner.spawn();
const emu = new IdmEmulator(new IdmConfig({ sync: { mappings: [] }, managed: { objects: [{ name: "w", schema: { properties: {} } }] } }), runner);
let release!: () => void;
const gate = new Promise<void>((resolve) => { release = resolve; });
let finish!: () => void;
const done = new Promise<void>((resolve) => { finish = resolve; });
shared.rhinoBackstopLate = { release, done, runner };
it("original late owner", () => {
  setTimeout(async () => {
    await gate;
    await emu.request({ method: "GET", path: "managed/w", query: { _queryId: "late missing" } });
    finish();
  }, 0);
});
