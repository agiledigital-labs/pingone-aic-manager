import { spawn } from "node:child_process";
import { mkdtemp, open, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

export async function runChild(scenario: string, pool: string, isolate: boolean, extra: Record<string, string> = {}, args: string[] = []) {
  const directory = await mkdtemp(join(tmpdir(), "rhino-backstop-"));
  const report = join(directory, "report.json");
  const environment = Object.fromEntries(Object.entries(process.env).filter(([key]) => !key.startsWith("VITEST") && !key.startsWith("VITE_")));
  try {
    const output = join(directory, "output.log");
    const log = await open(output, "w");
    let code: number | null;
    try {
      const child = spawn(process.execPath, [fileURLToPath(new URL("vitest.mjs", import.meta.resolve("vitest/package.json"))), "run", "--config", "test/fixtures/emulation-backstop/config.ts", ...args], {
        cwd: process.cwd(), stdio: ["ignore", log.fd, log.fd],
        env: { ...environment, AIC_SCRIPT_TESTER_JVM: "host", NO_COLOR: "1", R5_CASE: scenario, R5_POOL: pool, R5_ISOLATE: String(isolate), R5_REPORT: report, ...extra },
      });
      const timer = setTimeout(() => child.kill("SIGKILL"), 60_000);
      try { code = await new Promise<number | null>((resolve, reject) => { child.once("error", reject); child.once("exit", resolve); }); }
      finally { clearTimeout(timer); }
    } finally { await log.close(); }
    const result = { code, output: await readFile(output, "utf8") };
    return { ...result, report: await readFile(report, "utf8").catch((error: NodeJS.ErrnoException) => { if (error.code === "ENOENT") return ""; throw error; }) };
  } finally { await rm(directory, { recursive: true, force: true }); }
}
