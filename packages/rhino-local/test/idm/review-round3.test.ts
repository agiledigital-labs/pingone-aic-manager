/** Round-three false passes, exercised through real pinned Rhino processes. */
import { afterAll, beforeAll, expect, it } from "vitest";
import { HostCallError, RhinoRunner } from "../../src/runner.ts";

import { expectEmulationFailure } from "../../src/harness/vitest.ts";
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
  let release!: () => void;
  const gate = new Promise<null>((resolve) => { release = () => resolve(null); });
  const result = await runner.eval({ engine: "idm", timeoutMs: 80, source }, { onCall: () => gate });
  release();
  await gate;
  expect(result.outcome).toBe("timeout");
  expect(result.emulation).toMatchObject({ kind: "timeout", message: "IDM harness timeout" });
  expectEmulationFailure("IDM harness timeout");
});

it("R3 #1 AM timeout does not claim to be IDM", async () => {
  const result = await runner.eval({ timeoutMs: 80, source: "while(true){}" });
  expect(result.outcome).toBe("timeout");
  expect(result.emulation?.message).toBe("Rhino harness timeout");
  expectEmulationFailure("Rhino harness timeout");
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
  expectEmulationFailure("named _queryId: missing");
});

it("R3 #3 a second runner cannot hide a fatal nested result from the first", async () => {
  const second = await RhinoRunner.spawn();
  try {
    const [outer, independent] = await Promise.all([
      runner.eval({ engine: "idm", source: "try{openidm.read('outer')}catch(e){} 'passed';" }, {
        onCall: async () => {
          const nested = await second.eval({ engine: "idm", source: "require('unsupported');" });
          expect(nested.emulation?.kind).toBe("missing");
          return 42; // Adapter deliberately ignores the nested verdict.
        },
      }),
      second.eval({ engine: "idm", source: "openidm.read('independent');" }, { onCall: () => 7 }),
    ]);
    expect(outer.outcome).not.toBe("ok");
    expect(outer.emulation?.kind).toBe("missing");
    expect(outer.missing).toContain("require");
    expectEmulationFailure(/require/);
    expect(independent).toMatchObject({ outcome: "ok", value: 7 });
  } finally { await second.close(); }
}, 30_000);

it("R3 #4 emulator journal survives ignored results, fork calls and snapshot edits", async () => {
  const emu = emulator(runner);
  const child = emu.fork();
  await emu.request({ method: "GET", path: "managed/widget", query: { _queryId: "missing" } });
  await child.hostRequest({ type: "call", id: "guard-host", seq: 1, method: "query", args: ["managed/widget", { _queryId: "missing" }] });
  expect(emu.emulationFailures).toMatchObject([
    { failure: { kind: "missing", missing: ["named _queryId: missing"] } },
    { failure: { kind: "missing", missing: ["named _queryId: missing"] } },
  ]);
  expect(child.emulationFailures).toEqual(emu.emulationFailures);
  const snapshot = emu.emulationFailures;
  snapshot[0]!.failure.missing!.push("snapshot edit");
  expect(child.emulationFailures[0]?.failure.missing).not.toContain("snapshot edit");
  expect("resetEmulationFailures" in child).toBe(false);
  expect(emu.emulationFailures).toHaveLength(2);
  const independent = emulator(runner);
  await child.request({ method: "GET", path: "managed/widget", query: { _queryId: "again" } });
  expect(emu.emulationFailures).toHaveLength(3);
  expect(independent.emulationFailures).toEqual([]);
  expectEmulationFailure("named _queryId: missing");
  expectEmulationFailure("named _queryId: again");
});

it.each(["missing", "timeout", "protocol", "internal"] as const)("R3 #4 runner journals %s even when the result is ignored", async (kind) => {
  const start = runner.emulationFailures.length;
  const source = kind === "missing" ? "require('unsupported');" : kind === "timeout" ? "while(true){}" : "openidm.read('probe');";
  await runner.eval({ engine: "idm", timeoutMs: 80, source }, kind === "internal" ? { onCall: () => { throw new Error("adapter broke"); } } : {});
  expect(runner.emulationFailures.slice(start)).toEqual(expect.arrayContaining([expect.objectContaining({ failure: expect.objectContaining({ kind }) })]));
  const count = runner.emulationFailures.length;
  expectEmulationFailure(kind === "missing" ? /require/ : kind === "timeout" ? /IDM harness timeout/ : kind === "protocol" ? /rhino-local: no onCall handler for IDM job/ : /adapter broke/);
  await runner.eval({ engine: "idm", source: "throw {code:501,message:'tenant'};" });
  expect(runner.emulationFailures).toHaveLength(count);
});

it.each(["duplicate", "closed"] as const)("R3 #4 runner journals %s job rejection before returning", async (reason) => {
  const owned = await RhinoRunner.spawn();
  const id = `round3-journal-${reason}`;
  try {
    if (reason === "closed") {
      await owned.close();
      await expect(owned.eval({ id, source: "42" })).rejects.toThrow(/closing/);
    } else {
      await owned.eval({ id, engine: "idm", source: "openidm.read('outer');" }, {
        onCall: async () => {
          await expect(owned.eval({ id, source: "42" })).rejects.toThrow(/duplicate/);
          return 42;
        },
      });
    }
    expectEmulationFailure(reason === "closed" ? /rhino-local: runner is closing/ : /rhino-local: duplicate active job id: .+/);
    expect(owned.emulationFailures).toEqual(expect.arrayContaining([
      expect.objectContaining({ context: `job ${id}`, failure: expect.objectContaining({ kind: "protocol" }) }),
    ]));
  } finally { await owned.close(); }
}, 30_000);
