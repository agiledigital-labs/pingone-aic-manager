#!/usr/bin/env node
/**
 * List failed AIC-lane tests and fetch their logs.
 *
 *   aic-script-tester-show-log [--stdout] [--provider-module <file>]
 *
 * The logs are written to `.aic-script-tester/failures/latest-logs.json` (0600) and
 * opened in LOGS_EDITOR or EDITOR; `--stdout` prints them instead. Logs come
 * from the configured tenant provider (see `--provider-module`), which must
 * have log access: log keys, or the `aic` CLI. Fetches one transaction at a
 * time — never a range query.
 */
import { parseArgs } from "node:util";
import { createDefaultIo, runShowLog } from "../src/harness/show-log.ts";
import { loadProviderModule } from "../src/provider-module.ts";

try {
  const { values } = parseArgs({
    options: {
      stdout: { type: "boolean", default: false },
      "provider-module": { type: "string" },
    },
  });
  if (values["provider-module"] !== undefined) {
    await loadProviderModule(values["provider-module"]);
  }
  process.exitCode = await runShowLog(createDefaultIo(), { stdout: values.stdout });
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error));
  process.exit(1);
}
