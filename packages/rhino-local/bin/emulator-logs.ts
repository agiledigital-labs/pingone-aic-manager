#!/usr/bin/env node
/**
 * Serve the emulated monitoring-logs API (`GET /monitoring/logs`).
 *
 *   aic-emulator-logs [--port N] [--host H] [--seed records.jsonl]
 *                     [--retention-days N] [--requests-per-minute N|off]
 *
 * Keys come from the environment: API_KEY_ID and API_KEY_SECRET (the names
 * kikori-aic reads). KIKORI_CLOCK, when set, names the shared virtual-clock
 * file. `--seed` loads one JSON `LogRecord` per line; a producer in the same
 * process can instead use `startLogsServer` from the package's `./logs` entry
 * and emit into its `LogStore`.
 */
import { readFileSync } from "node:fs";
import { parseArgs } from "node:util";
import { clockFromEnv } from "../src/logs/clock.ts";
import { startLogsServer } from "../src/logs/server.ts";
import { LogStore } from "../src/logs/store.ts";
import type { LogRecord } from "../src/logs/record.ts";

try {
  const { values } = parseArgs({
    options: {
      port: { type: "string", default: "8080" },
      host: { type: "string", default: "127.0.0.1" },
      seed: { type: "string" },
      "retention-days": { type: "string", default: "30" },
      "requests-per-minute": { type: "string", default: "60" },
    },
  });
  const id = process.env["API_KEY_ID"];
  const secret = process.env["API_KEY_SECRET"];
  if (id === undefined || id === "" || secret === undefined || secret === "") {
    throw new Error("set API_KEY_ID and API_KEY_SECRET");
  }
  const days = Number(values["retention-days"]);
  if (!Number.isInteger(days) || days < 1) {
    throw new Error("--retention-days must be a positive integer");
  }
  const rpmText = values["requests-per-minute"];
  const rpm = rpmText === "off" ? false : Number(rpmText);
  if (rpm !== false && (!Number.isInteger(rpm) || rpm < 1)) {
    throw new Error("--requests-per-minute must be a positive integer or off");
  }
  const port = Number(values.port);
  if (!Number.isInteger(port) || port < 0 || port > 65535) {
    throw new Error("--port must be 0-65535");
  }
  const clock = clockFromEnv();
  const store = new LogStore({ clock, retentionNs: BigInt(days) * 24n * 3600n * 1_000_000_000n });
  if (values.seed !== undefined) {
    for (const line of readFileSync(values.seed, "utf8").split("\n")) {
      if (line.trim() !== "") store.emit(JSON.parse(line) as LogRecord);
    }
  }
  const server = await startLogsServer({
    store,
    clock,
    keys: { id, secret },
    port,
    host: values.host,
    requestsPerMinute: rpm,
  });
  console.error(`aic-emulator-logs listening on ${server.url}`);
  const stop = (): void => {
    void server.close().then(() => process.exit(0));
  };
  process.on("SIGINT", stop);
  process.on("SIGTERM", stop);
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error));
  process.exit(1);
}
