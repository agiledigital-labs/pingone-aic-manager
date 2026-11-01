import { expect, it } from "vitest";
import type { LateState } from "./late-a.fixture.ts";
it("clean later file", async () => {
  const state = (process as typeof process & { rhinoBackstopLate?: LateState }).rhinoBackstopLate!;
  state.release(); await state.done;
  expect(await state.runner.eval({ engine: "idm", source: "42" })).toMatchObject({ outcome: "ok", value: 42 });
  await state.runner.close();
});
