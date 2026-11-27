/** End to end: the driver writes into a real LogStore, and a client pulls it back as kikori-aic does. */
import { afterAll, beforeAll, expect, it } from "vitest";
import { RhinoRunner } from "../../src/runner.ts";
import { Driver, manualPacer } from "../../src/demo/driver.ts";
import { parseMix } from "../../src/demo/mix.ts";
import { LogStore } from "../../src/logs/store.ts";
import { startLogsServer } from "../../src/logs/server.ts";
import type { Clock } from "../../src/logs/clock.ts";
import type { LogRecord } from "../../src/logs/record.ts";
import { formatRfc3339Ns } from "../../src/logs/time.ts";
import { config, makeEngineFor, mixJson, payload, T0, HOUR, ts } from "./rig.ts";

let runner: RhinoRunner;
beforeAll(async () => {
  runner = await RhinoRunner.spawn();
}, 60_000);
afterAll(async () => {
  await runner?.close();
});

const SECOND = 1_000_000_000n;

it("a driven journey's records come back through GET /monitoring/logs, paged, bounds (begin, end]", async () => {
  const clock = { now: T0 };
  const fake: Clock = { nowNs: () => clock.now };
  const store = new LogStore({ clock: fake });
  const server = await startLogsServer({ store, clock: fake, keys: { id: "k", secret: "s" }, requestsPerMinute: false });
  try {
    const mix = parseMix(mixJson({ perHour: 300, requests: [
      { name: "token", stream: "am-access", method: "POST", path: "/am/oauth2/realms/root/realms/alpha/access_token", rate: { perHour: 3000 },
        networks: [{ cidr: "198.51.100.0/24", share: 1 }], status: [{ share: 1, status: "SUCCESSFUL", statusCode: 200 }] },
    ] }));
    const cfg = config();
    const end = T0 + HOUR;
    const driver = new Driver({ mix, config: cfg, makeEngine: makeEngineFor(runner, cfg), sink: store, pacer: manualPacer(T0), seed: "e2e" });
    await driver.run({ until: end });
    clock.now = end + 5n * SECOND;

    const pulled: LogRecord[] = [];
    let cookie: string | null = null;
    let pages = 0;
    do {
      const q = new URLSearchParams({
        source: "am-everything",
        beginTime: formatRfc3339Ns(T0).replace(/\.\d+Z$/, "Z"),
        endTime: formatRfc3339Ns(end).replace(/\.\d+Z$/, "Z"),
        _pageSize: "1000",
      });
      if (cookie !== null) q.set("_pagedResultsCookie", cookie);
      const res = await fetch(`${server.url}/monitoring/logs?${q}`, { headers: { "x-api-key": "k", "x-api-secret": "s" } });
      expect(res.status).toBe(200);
      const body = (await res.json()) as { result: LogRecord[]; pagedResultsCookie: string | null };
      pulled.push(...body.result);
      cookie = body.pagedResultsCookie;
      pages++;
    } while (cookie !== null);

    expect(pages).toBeGreaterThanOrEqual(2);
    expect(pulled.some((r) => ts(r) === T0)).toBe(false); // nothing at the handoff instant
    expect(pulled.every((r) => ts(r) > T0 && ts(r) <= end)).toBe(true);
    // A driven journey: a tree-completed record and its outcome came back by transaction id.
    const trees = pulled.filter((r) => payload(r)["eventName"] === "AM-TREE-LOGIN-COMPLETED");
    expect(trees.length).toBeGreaterThan(20);
    const tx = payload(trees[0]!)["transactionId"];
    expect(pulled.some((r) => payload(r)["eventName"] === "AM-ACCESS-OUTCOME" && payload(r)["transactionId"] === tx)).toBe(true);
  } finally {
    await server.close();
  }
}, 120_000);
