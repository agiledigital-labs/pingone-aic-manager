/** Round-three false passes, exercised through real pinned Rhino processes. */
import { afterAll, beforeAll, expect, it } from "vitest";
import { HostCallError, RhinoRunner } from "../../src/runner.ts";

import { EmulationFailure } from "../../src/emulation.ts";
import { emulator } from "./fixtures.ts";

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

it("R3 #2 REST adapter preserves missing as fatal rather than tenant 501", async () => {
  const emu = emulator(runner);
  const result = await runner.eval({ engine: "idm", source: "try{openidm.read('probe')}catch(e){} ({passed:true});" }, {
    onCall: async () => {
      const response = await emu.request({ method: "GET", path: "managed/widget", query: { _queryId: "missing" } });
      expect(response.status).toBe(501); // Retain diagnostic REST data.
      if (response.emulation) throw new EmulationFailure(response.emulation);
      if (response.status >= 400) throw new HostCallError({ class: "example.Rest", code: response.status,
        message: "REST failure", detail: response.body });
      return response.body;
    },
  });
  expect(result.outcome).not.toBe("ok");
  expect(result.emulation?.kind).toBe("missing");
  expect(result.missing).toContain("named _queryId: missing");
});
