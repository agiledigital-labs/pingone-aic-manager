import { execFile } from "node:child_process";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";
import { beforeAll, expect, it } from "vitest";

let assertions: Array<{ title: string; status: string; failureMessages: string[] }>;
beforeAll(async () => {
  const childEnv = Object.fromEntries(Object.entries(process.env).filter(([key]) => !key.startsWith("VITEST") && !key.startsWith("VITE_")));
  const directory = await mkdtemp(join(tmpdir(), "rhino-owner-guard-"));
  const report = join(directory, "report.json");
  try {
    await promisify(execFile)(process.execPath, [fileURLToPath(new URL("vitest.mjs", import.meta.resolve("vitest/package.json"))), "run", "--reporter=json", `--outputFile=${report}`, "--config", "test/fixtures/emulation-ownership/config.ts"], {
      cwd: process.cwd(), timeout: 60_000, env: { ...childEnv, AIC_SCRIPT_TESTER_JVM: "host", NO_COLOR: "1" },
    }).catch((error: { code: number }) => { expect(error.code).toBe(1); });
    const result = JSON.parse(await readFile(report, "utf8")) as { testResults: Array<{ assertionResults: typeof assertions }> };
    assertions = result.testResults.flatMap((file) => file.assertionResults);
  } finally { await rm(directory, { recursive: true, force: true }); }
}, 65_000);

it.each([
  ["detached child", "failed", "spawned thread outlived its job"],
  ["unawaited eval", "failed", "outstanding work at test end"],
  ["clean after late eval", "passed", ""],
  ["unfinished callback after timeout", "failed", "outstanding work at test end"],
  ["clean after late callback", "passed", ""],
  ["reset cannot erase", "failed", "named _queryId: missing"],
  ["replace fixture", "failed", "named _queryId: missing"],
  ["clear fixture", "failed", "named _queryId: missing"],
  ["sequential failing sibling", "failed", "named _queryId: missing"],
  ["sequential clean sibling", "passed", ""],
  ["concurrent failing sibling", "failed", "concurrent missing"],
  ["concurrent clean sibling", "passed", ""],
  ["named expected failure retained", "passed", ""],
  ["absent expected failure", "failed", "expected emulation failure did not occur"],
  ["unnamed source", "failed", "named _queryId: missing"],
  ["unawaited request", "failed", "outstanding work at test end"],
  ["ordinary tenant 501", "passed", ""],
  ["duplicate module registry", "passed", ""],
])("R4 lifecycle verdict: %s", (title, status, cause) => {
  const assertion = assertions.find((entry) => entry.title === title);
  expect(assertion, JSON.stringify(assertions)).toBeDefined();
  expect(assertion?.status, JSON.stringify(assertion)).toBe(status);
  if (cause) expect(assertion?.failureMessages.join("\n")).toContain(cause);
});
