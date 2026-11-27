/** The demo's exit status and final report, from a real driver run over the real engine. */
import { afterAll, beforeAll, expect, it } from "vitest";
import { RhinoRunner } from "../../src/runner.ts";
import { JourneyConfig } from "../../src/journey/index.ts";
import { parseMix } from "../../src/demo/mix.ts";
import { exitCode, finalReport, statusLine } from "../../src/demo/summary.ts";
import { expectEmulationFailure } from "../../src/harness/emulation-guard.ts";
import { SUCCESS_NODE_ID } from "../../src/aic/constants.ts";
import { MINUTE, mixJson, runDriver } from "./rig.ts";

let runner: RhinoRunner;
beforeAll(async () => {
  runner = await RhinoRunner.spawn();
}, 60_000);
afterAll(async () => {
  await runner?.close();
});

/** tree-a whose entry node is a type the emulator does not model: every journey is a 501. */
const unmodelled = (): JourneyConfig =>
  new JourneyConfig({
    realm: "alpha",
    trees: {
      "tree-a": {
        entryNodeId: "odd",
        nodes: { odd: { nodeType: "InventedNode", connections: { outcome: SUCCESS_NODE_ID } } },
      },
    },
    nodes: { InventedNode: { odd: {} } },
    scripts: {},
  });
const journeysOnly = () => parseMix({ ...(mixJson({ perHour: 600 }) as object), requests: [] });

it("a run whose journeys the engine answered 501 exits 1 and reports them", async () => {
  expectEmulationFailure("node type: InventedNode");
  const { report } = await runDriver(runner, {
    config: unmodelled(),
    mix: journeysOnly(),
    duration: 5n * MINUTE,
  });
  expect(report.journeys.errored).toBeGreaterThan(0);
  expect(report.journeys.erroredByStatus).toEqual({ "501": report.journeys.errored });
  expect(exitCode(report)).toBe(1);
  const text = finalReport(report);
  expect(text).toContain(`errored with HTTP 501: ${report.journeys.errored}`);
  expect(text).toContain("node type: InventedNode");
  expect(statusLine(report, report.toNs)).toContain(`errored ${report.journeys.errored}`);
}, 120_000);

it("a clean run exits 0", async () => {
  const { report } = await runDriver(runner, { mix: journeysOnly(), duration: 5n * MINUTE });
  expect(report.journeys.started).toBeGreaterThan(0);
  expect(report.journeys.errored).toBe(0);
  expect(exitCode(report)).toBe(0);
}, 120_000);
