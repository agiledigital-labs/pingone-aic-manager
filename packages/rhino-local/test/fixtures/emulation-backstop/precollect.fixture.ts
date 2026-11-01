import { afterAll, expect, it } from "vitest";
import { RhinoRunner } from "../../../src/runner.ts";
const runner = await RhinoRunner.spawn();
afterAll(() => runner.close());
it("precollection helper initialized before the real job", async () => {
  expect((globalThis as typeof globalThis & { precollectHelper?: string }).precollectHelper).toBe("initialized");
  expect(await runner.eval({ engine: "idm", source: "42" })).toMatchObject({ outcome: "ok", value: 42 });
});
