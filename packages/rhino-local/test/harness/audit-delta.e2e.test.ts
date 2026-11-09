/**
 * Item 12: the worker sends acknowledged audit deltas to one main-process
 * ledger instead of re-sending its whole audit at every boundary. The
 * contract is that the run's verdict is unchanged, so this runs one nested
 * suite (test/fixtures/audit-delta: five files over two fork workers, with
 * faults in four, a declared fault, a worker that SIGKILLs itself after its
 * fault was recorded, and a test that times out with its case still running)
 * twice — once with deltas, once with AIC_SCRIPT_TESTER_AUDIT_SNAPSHOTS=1,
 * the whole-audit transport older releases used — and requires the same
 * tally, statuses, exit code and backstop lines from both.
 *
 * The control that the comparison can fail: the dying file's fault must be
 * in both tallies. It was sent before the kill and nothing after it was, so
 * a ledger that only kept what a file's final publish carried would lose it.
 */
import { spawn } from "node:child_process";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { expect, it } from "vitest";
import type { FaultTally } from "../../src/harness/fault-tally.ts";

interface JsonReport {
  testResults: Array<{ name: string; status: string; assertionResults: Array<{ fullName: string; status: string }> }>;
}

interface Run {
  code: number | null;
  tally: FaultTally;
  statuses: Record<string, string>;
  files: Record<string, string>;
  backstop: string[];
  output: string;
}

async function run(dir: string, label: string, extra: Record<string, string>): Promise<Run> {
  const report = join(dir, `${label}.json`);
  const tallyPath = join(dir, `${label}-tally.json`);
  const env = Object.fromEntries(
    Object.entries(process.env).filter(([key]) => !key.startsWith("VITEST") && !key.startsWith("VITE_"))
  );
  delete env.AIC_SCRIPT_TESTER_JVM_POOL;
  delete env.AIC_SCRIPT_TESTER_AUDIT_SNAPSHOTS;
  const vitest = fileURLToPath(new URL("vitest.mjs", import.meta.resolve("vitest/package.json")));
  const child = spawn(process.execPath, [vitest, "run", "--config", "test/fixtures/audit-delta/config.ts"], {
    cwd: process.cwd(),
    stdio: ["ignore", "pipe", "pipe"],
    env: { ...env, ...extra, AIC_SCRIPT_TESTER_JVM: "host", NO_COLOR: "1", AUDIT_DELTA_REPORT: report, AIC_SCRIPT_TESTER_FAULT_TALLY: tallyPath },
  });
  let output = "";
  child.stdout.on("data", (chunk: Buffer) => (output += chunk.toString()));
  child.stderr.on("data", (chunk: Buffer) => (output += chunk.toString()));
  const code = await new Promise<number | null>((resolve, reject) => {
    const timer = setTimeout(() => child.kill("SIGKILL"), 90_000);
    child.once("error", reject);
    child.once("exit", (exit) => {
      clearTimeout(timer);
      resolve(exit);
    });
  });
  const results = JSON.parse(await readFile(report, "utf8")) as JsonReport;
  const root = process.cwd();
  return {
    code,
    tally: JSON.parse(await readFile(tallyPath, "utf8")) as FaultTally,
    statuses: Object.fromEntries(results.testResults.flatMap((file) => file.assertionResults.map((test) => [test.fullName, test.status]))),
    files: Object.fromEntries(results.testResults.map((file) => [file.name.replace(`${root}/`, ""), file.status])),
    backstop: output
      .split("\n")
      .filter((line) => line.includes("Emulation run backstop"))
      .map((line) => line.replace(/[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/g, "<uuid>"))
      .sort(),
    output,
  };
}

it("acknowledged deltas give the same verdict as whole-audit snapshots, a dying worker included", async () => {
  const dir = await mkdtemp(join(tmpdir(), "rhino-audit-delta-"));
  try {
    const delta = await run(dir, "delta", {});
    const snapshot = await run(dir, "snapshot", { AIC_SCRIPT_TESTER_AUDIT_SNAPSHOTS: "1" });
    const both = `--- delta\n${delta.output}\n--- snapshot\n${snapshot.output}`;

    expect(delta.tally, both).toEqual(snapshot.tally);
    expect(delta.statuses, both).toEqual(snapshot.statuses);
    expect(delta.files, both).toEqual(snapshot.files);
    expect(delta.code, both).toBe(snapshot.code);
    expect(delta.backstop, both).toEqual(snapshot.backstop);

    // The comparison means something: the run failed, faults from several
    // files were tallied, and the file whose worker died is among them.
    expect(delta.code, both).toBe(1);
    const dies = "test/fixtures/audit-delta/dies.fixture.ts";
    const date = delta.tally.faults.find((fault) => fault.id === "script-defect/send-body-js-date");
    expect(date?.files, both).toEqual(expect.arrayContaining(["test/fixtures/audit-delta/a.fixture.ts", "test/fixtures/audit-delta/b.fixture.ts", dies]));
    expect(delta.tally.faults.find((fault) => fault.id === "state-put-undefined")?.files, both).toEqual(
      expect.arrayContaining(["test/fixtures/audit-delta/a.fixture.ts", "test/fixtures/audit-delta/c.fixture.ts"])
    );
    // The declared faults passed in both runs; the undeclared ones failed.
    expect(delta.statuses["a declared runs"], both).toBe("passed");
    expect(delta.statuses["c declared runs"], both).toBe("passed");
    expect(delta.statuses["a date runs"], both).toBe("failed");
    expect(delta.statuses["c clean runs"], both).toBe("passed");
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
}, 200_000);
