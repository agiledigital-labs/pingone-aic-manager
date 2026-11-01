/** Round-three false passes, exercised through real pinned Rhino processes. */
import { afterAll, beforeAll, expect, it } from "vitest";
import { RhinoRunner } from "../../src/runner.ts";

let runner: RhinoRunner;
beforeAll(async () => { runner = await RhinoRunner.spawn(); }, 60_000);
afterAll(async () => { await runner?.close(); }, 30_000);

it.each([
  ["finally return", "function f(){try{openidm.read('wait')}finally{return 42}} f()"],
  ["spawn host wait", "spawn(function(){openidm.read('wait')}).join();42"],
  ["spawn instruction observer", "spawn(function(){while(true){}}).join();42"],
])("R3 #1 %s cannot swallow a harness timeout", async (_name, source) => {
  const result = await runner.eval({ engine: "idm", timeoutMs: 80, source }, {
    onCall: () => new Promise<never>(() => {}),
  });
  expect(result.outcome).toBe("timeout");
  expect(result.emulation).toMatchObject({ kind: "timeout", message: "IDM harness timeout" });
});

it("R3 #1 AM timeout does not claim to be IDM", async () => {
  const result = await runner.eval({ timeoutMs: 80, source: "while(true){}" });
  expect(result.outcome).toBe("timeout");
  expect(result.emulation?.message).toBe("Rhino harness timeout");
});
