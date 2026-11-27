/**
 * The demo bin itself (review round 1 P2-4): the process's exit status and final report, not
 * only the helpers' return values. Each run spawns the real bin over the real engine, lets it
 * drive a few journeys on the real clock, and stops it with SIGINT as an operator would.
 */
import { spawn, spawnSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterAll, expect, it } from "vitest";
import { configInput, mixJson } from "./rig.ts";

const BIN = join(import.meta.dirname, "../../bin/emulator-demo.ts");
const dir = mkdtempSync(join(tmpdir(), "emulator-demo-bin-"));
afterAll(() => rmSync(dir, { recursive: true, force: true }));

/** The test's environment without a virtual-clock file, so the bin runs on the real clock. */
function withoutClock(): NodeJS.ProcessEnv {
  const env = { ...process.env };
  delete env["KIKORI_CLOCK"];
  return env;
}
/** Runs the bin until it has driven journeys for `ms`, then SIGINTs it. */
async function runBin(name: string, journeys: unknown, ms = 4000): Promise<{ code: number | null; stderr: string }> {
  const journeysPath = join(dir, `${name}-journeys.json`);
  const mixPath = join(dir, `${name}-mix.json`);
  writeFileSync(journeysPath, JSON.stringify(journeys));
  // 36,000 an hour is ten a second, so a few seconds always start journeys; no other traffic.
  writeFileSync(mixPath, JSON.stringify({ ...(mixJson({ perHour: 36_000, abandon: 0 }) as object), requests: [] }));
  const child = spawn(
    process.execPath,
    [BIN, "--journeys", journeysPath, "--mix", mixPath, "--port", "0", "--requests-per-minute", "off"],
    {
      env: { ...withoutClock(), API_KEY_ID: "id", API_KEY_SECRET: "secret" },
      stdio: ["ignore", "ignore", "pipe"],
    },
  );
  let stderr = "";
  child.stderr.on("data", (chunk: Buffer) => {
    stderr += chunk.toString();
  });
  const exited = new Promise<number | null>((resolve) => child.on("exit", (code) => resolve(code)));
  await new Promise<void>((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error(`bin never listened:\n${stderr}`)), 60_000);
    child.stderr.on("data", () => {
      if (stderr.includes("listening on")) {
        clearTimeout(timer);
        resolve();
      }
    });
  });
  await new Promise((resolve) => setTimeout(resolve, ms));
  child.kill("SIGINT");
  return { code: await exited, stderr };
}

it("exits 1 and prints the final report when the engine answered 400", async () => {
  // tree-a is an inner-tree-only tree, which the tenant (and so the engine) refuses with 400.
  const input = configInput();
  const tree = input.trees["tree-a"];
  if (tree === undefined) throw new Error("rig has no tree-a");
  const { code, stderr } = await runBin("bad", { ...input, trees: { "tree-a": { ...tree, innerTreeOnly: true } } });
  expect(stderr).toMatch(/errored with HTTP 400: [1-9]/);
  expect(code).toBe(1);
}, 120_000);

it("exits 0 when every journey ran (the discriminating case for a bin that always fails)", async () => {
  const { code, stderr } = await runBin("good", configInput());
  expect(stderr).toMatch(/journeys [1-9]\d*: ok [1-9]/);
  expect(stderr).not.toMatch(/errored with HTTP/);
  expect(code).toBe(0);
}, 120_000);

// Astra review P2-2: an unusable concurrency is refused at the command line, before anything runs.
// Discriminating: 0 stalled the driver and "x" (NaN) removed the bound.
it.each(["0", "x", "1.5", "-2"])("refuses --concurrency %s with a named error", (value) => {
  const journeysPath = join(dir, "conc-journeys.json");
  const mixPath = join(dir, "conc-mix.json");
  writeFileSync(journeysPath, JSON.stringify(configInput()));
  writeFileSync(mixPath, JSON.stringify(mixJson()));
  const run = spawnSync(
    process.execPath,
    [BIN, "--journeys", journeysPath, "--mix", mixPath, "--port", "0", `--concurrency=${value}`],
    { env: { ...withoutClock(), API_KEY_ID: "id", API_KEY_SECRET: "secret" }, encoding: "utf8", timeout: 60_000 },
  );
  expect(run.status).toBe(1);
  expect(run.stderr).toContain("--concurrency must be a positive integer");
  expect(run.stderr).not.toContain("listening on");
});
