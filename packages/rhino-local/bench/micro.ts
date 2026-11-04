/**
 * Eval-level benchmark, the shape of the client's own measurement: one
 * runner, warmed up, then the same decision-node job with progressively more
 * of the case preamble.
 *
 *     node bench/micro.ts
 */
import { runCase } from "../src/bindings/index.ts";
import { RhinoRunner } from "../src/runner.ts";
import type { Case } from "../src/case/types.ts";
import { benchLibraries, NO_REQUIRE, USES_COMMON } from "./fixtures.ts";

const ROUNDS = 60;
const WARMUP = 40;

const runner = await RhinoRunner.spawn();
const libraries = benchLibraries();
console.log(`library map: ${Object.keys(libraries).length} bodies, ${(JSON.stringify(libraries).length / 1024).toFixed(0)} KB`);

function kase(script: string, withLibraries: boolean): Case {
  return {
    name: "micro",
    script,
    given: withLibraries ? { libraries, callbacks: [] } : { callbacks: [] },
    expect: { outcome: "done" },
  } as Case;
}

async function time(label: string, fn: () => Promise<unknown>): Promise<void> {
  for (let i = 0; i < WARMUP; i += 1) await fn();
  const xs: number[] = [];
  for (let i = 0; i < ROUNDS; i += 1) {
    const t = performance.now();
    await fn();
    xs.push(performance.now() - t);
  }
  xs.sort((a, b) => a - b);
  console.log(`${label.padEnd(36)} median ${xs[ROUNDS >> 1]!.toFixed(2)} ms  p90 ${xs[Math.floor(ROUNDS * 0.9)]!.toFixed(2)} ms`);
}

try {
  await time("bare eval (no preamble)", () => runner.eval({ source: "1 + 1", sourceName: "bare.js" }));
  await time("preamble, no libraries", () => runCase(runner, kase(NO_REQUIRE, false)));
  await time("preamble + libraries, none required", () => runCase(runner, kase(NO_REQUIRE, true)));
  await time("preamble + libraries, lib-common", () => runCase(runner, kase(USES_COMMON, true)));
} finally {
  await runner.close();
}
