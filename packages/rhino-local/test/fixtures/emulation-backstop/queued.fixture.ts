import { afterAll, expect, it } from "vitest";
import { RhinoRunner } from "../../../src/runner.ts";
const runner = await RhinoRunner.spawn();
afterAll(() => runner.close());
it("empty successful test", () => {});
it("client setup and actual runner coexist with queued reporter", async () => {
  expect((globalThis as typeof globalThis & { clientSetupRan?: boolean }).clientSetupRan).toBe(true);
  expect(await runner.eval({ engine: "idm", source: "42" })).toMatchObject({ outcome: "ok", value: 42 });
});
