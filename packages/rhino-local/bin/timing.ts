#!/usr/bin/env node
/**
 * Summarise an AIC_SCRIPT_TESTER_TIMING file: per field, count, sum, p50, p90
 * and max, over every spawn, borrow and job row.
 *
 *   AIC_SCRIPT_TESTER_TIMING=timing.jsonl npx vitest run
 *   npx aic-script-tester-timing timing.jsonl
 *
 * `--json` prints the summary as JSON instead of a table.
 */
import { readFileSync } from "node:fs";
import { formatTimingSummary, summarizeTiming } from "../src/timing.ts";

const args = process.argv.slice(2);
const json = args.includes("--json");
const files = args.filter((a) => a !== "--json");
if (files.length !== 1) {
  process.stderr.write("usage: aic-script-tester-timing [--json] <timing.jsonl>\n");
  process.exit(2);
}
const summary = summarizeTiming(readFileSync(files[0] as string, "utf8"));
process.stdout.write(json ? `${JSON.stringify(summary, null, 2)}\n` : formatTimingSummary(summary));
