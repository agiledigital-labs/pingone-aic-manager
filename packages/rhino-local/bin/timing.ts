#!/usr/bin/env node
/**
 * Summarise an AIC_SCRIPT_TESTER_TIMING file: per field, count, sum, p50, p90
 * and max, over every spawn, borrow and job row.
 *
 *   AIC_SCRIPT_TESTER_TIMING=timing.jsonl npx vitest run
 *   npx aic-script-tester-timing timing.jsonl
 *
 * `--json` prints the summary as JSON instead of a table. `--share` replaces
 * every library id and source name with a salted hash (random salt; give
 * `--salt=<text>` to make two summaries comparable), for a summary that can be
 * sent outside the project. Field names and numbers are kept.
 */
import { readFileSync } from "node:fs";
import { formatTimingSummary, summarizeTiming } from "../src/timing.ts";

const args = process.argv.slice(2);
const json = args.includes("--json");
const saltArg = args.find((a) => a.startsWith("--salt="));
const share = args.includes("--share") || saltArg !== undefined;
const files = args.filter((a) => a !== "--json" && a !== "--share" && !a.startsWith("--salt="));
if (files.length !== 1 || files[0]?.startsWith("--")) {
  process.stderr.write("usage: aic-script-tester-timing [--json] [--share [--salt=<text>]] <timing.jsonl>\n");
  process.exit(2);
}
const summary = summarizeTiming(readFileSync(files[0] as string, "utf8"),
  share ? { share: saltArg === undefined ? {} : { salt: saltArg.slice("--salt=".length) } } : {});
process.stdout.write(json ? `${JSON.stringify(summary, null, 2)}\n` : formatTimingSummary(summary));
