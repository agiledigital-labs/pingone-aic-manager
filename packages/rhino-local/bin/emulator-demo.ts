#!/usr/bin/env node
/**
 * Serve the emulated monitoring-logs API and drive demo traffic into it.
 *
 *   aic-emulator-demo --journeys journeys.json --mix mix.json [--breaks breaks.json]
 *                     [--port N] [--host H] [--seed S] [--scale X] [--concurrency N]
 *                     [--retention-days N] [--requests-per-minute N|off]
 *
 * Keys: API_KEY_ID and API_KEY_SECRET. Clock: KIKORI_CLOCK (a virtual-clock
 * file), else real time. See the README's "Demo driver" section for the file
 * formats. Runs until SIGINT or SIGTERM.
 */
import { readFileSync } from "node:fs";
import { parseArgs } from "node:util";
import { IdmConfig, IdmEmulator } from "../src/idm/index.ts";
import { JourneyEmulator } from "../src/journey/engine.ts";
import { clockFromEnv } from "../src/logs/clock.ts";
import { startLogsServer } from "../src/logs/server.ts";
import { LogStore } from "../src/logs/store.ts";
import { formatRfc3339Ns } from "../src/logs/time.ts";
import { parseBreaks } from "../src/demo/breaks.ts";
import { clockPacer, Driver } from "../src/demo/driver.ts";
import { loadJourneys } from "../src/demo/load.ts";
import { parseMix } from "../src/demo/mix.ts";
import { RhinoRunner } from "../src/runner.ts";

try {
  const { values } = parseArgs({
    options: {
      journeys: { type: "string" },
      mix: { type: "string" },
      breaks: { type: "string" },
      port: { type: "string", default: "8080" },
      host: { type: "string", default: "127.0.0.1" },
      seed: { type: "string", default: "demo" },
      scale: { type: "string", default: "1" },
      concurrency: { type: "string", default: "8" },
      "retention-days": { type: "string", default: "30" },
      "requests-per-minute": { type: "string", default: "60" },
    },
  });
  const id = process.env["API_KEY_ID"];
  const secret = process.env["API_KEY_SECRET"];
  if (!id || !secret) throw new Error("set API_KEY_ID and API_KEY_SECRET");
  if (values.journeys === undefined || values.mix === undefined) throw new Error("--journeys and --mix are required");
  const rpmText = values["requests-per-minute"];
  const rpm = rpmText === "off" ? false : Number(rpmText);
  if (rpm !== false && (!Number.isInteger(rpm) || rpm < 1)) throw new Error("--requests-per-minute must be a positive integer or off");
  const days = Number(values["retention-days"]);
  if (!Number.isInteger(days) || days < 1) throw new Error("--retention-days must be a positive integer");
  const scale = Number(values.scale);
  const concurrency = Number(values.concurrency);
  const port = Number(values.port);

  const mix = parseMix(readFileSync(values.mix, "utf8"));
  const breaks = values.breaks === undefined ? [] : parseBreaks(readFileSync(values.breaks, "utf8"));
  const { config, idm } = loadJourneys(values.journeys);
  const clock = clockFromEnv();
  const store = new LogStore({ clock, retentionNs: BigInt(days) * 24n * 3600n * 1_000_000_000n });
  const server = await startLogsServer({ store, clock, keys: { id, secret }, port, host: values.host, requestsPerMinute: rpm });
  const runner = await RhinoRunner.spawn();
  const idmEmulator = new IdmEmulator(new IdmConfig(idm), runner);
  const driver = new Driver({
    mix,
    config,
    makeEngine: (options) => new JourneyEmulator(config, runner, idmEmulator, options),
    sink: store,
    pacer: clockPacer(clock),
    seed: values.seed,
    breaks,
    scale,
    concurrency,
    onBehind: ({ lagNs, tag }) => console.error(`behind the clock by ${Number(lagNs / 1_000_000n) / 1000} virtual s on ${tag}`),
    onError: (error, tag) => console.error(`${tag}: ${error instanceof Error ? error.message : String(error)}`),
  });
  const stop = new AbortController();
  const started = clock.nowNs();
  console.error(`aic-emulator-demo listening on ${server.url}; traffic from ${formatRfc3339Ns(started)} (virtual)`);
  const status = setInterval(() => {
    const r = driver.report();
    console.error(
      `${formatRfc3339Ns(clock.nowNs())} journeys ${r.journeys.started} (ok ${r.journeys.succeeded}, failed ${r.journeys.failed}, ` +
        `abandoned ${r.journeys.abandoned}) records ${r.records.total} max lag ${Number(r.lag.overall.maxNs / 1_000_000n) / 1000} s`,
    );
  }, 30_000);
  const shutdown = (): void => stop.abort();
  process.on("SIGINT", shutdown);
  process.on("SIGTERM", shutdown);
  await driver.run({ signal: stop.signal });
  clearInterval(status);
  await server.close();
  await runner.close();
  process.exit(0);
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error));
  process.exit(1);
}
