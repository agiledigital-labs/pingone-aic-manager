import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { RhinoRunner } from "../../src/runner.ts";
import { parseBreaks } from "../../src/demo/breaks.ts";
import { Arrivals, clockPacer, Driver, manualPacer } from "../../src/demo/driver.ts";
import { Rng } from "../../src/demo/rng.ts";
import { parseMix } from "../../src/demo/mix.ts";
import { formatRfc3339Ns } from "../../src/logs/time.ts";
import { canon, config, HOUR, makeEngineFor, MINUTE, mixJson, payload, Recording, runDriver, T0, ts } from "./rig.ts";
import type { LogRecord } from "../../src/logs/record.ts";
import type { Clock } from "../../src/logs/clock.ts";

let runner: RhinoRunner;
beforeAll(async () => {
  runner = await RhinoRunner.spawn();
}, 60_000);
afterAll(async () => {
  await runner?.close();
});

const iso = (ns: bigint) => formatRfc3339Ns(ns);
const traffic = (): unknown => mixJson({ perHour: 600 });
/** A mix with no journeys, so no engine work: for the rate and break tests that only need requests. */
const requestsOnly = (perHour = 600): unknown => {
  const m = mixJson({ perHour }) as { journeys: unknown };
  return { ...m, journeys: {} };
};

describe("determinism", () => {
  it("same seed, same records; another seed, different ones", async () => {
    const mix = parseMix(traffic());
    const a = await runDriver(runner, { mix, duration: 20n * MINUTE, seed: "s1" });
    const b = await runDriver(runner, { mix, duration: 20n * MINUTE, seed: "s1", concurrency: 1 });
    const c = await runDriver(runner, { mix, duration: 20n * MINUTE, seed: "s2" });
    expect(a.records.length).toBeGreaterThan(300);
    expect(canon(b.records)).toEqual(canon(a.records));
    expect(canon(c.records)).not.toEqual(canon(a.records));
  }, 120_000);

  it("is independent of the concurrency bound (the discriminating case: 1 against 8)", async () => {
    const mix = parseMix(traffic());
    const one = await runDriver(runner, { mix, duration: 10n * MINUTE, concurrency: 1 });
    const many = await runDriver(runner, { mix, duration: 10n * MINUTE, concurrency: 16 });
    expect(canon(many.records)).toEqual(canon(one.records));
  }, 120_000);
});

describe("handoff", () => {
  it("writes nothing at or before the instant it starts, and the first record is after it", async () => {
    const { records, start } = await runDriver(runner, { mix: parseMix(traffic()), duration: 10n * MINUTE });
    expect(records.some((r) => ts(r) === start)).toBe(false);
    expect(records.every((r) => ts(r) > start)).toBe(true);
  }, 60_000);

  // Review round 3 P2-5: the gap floor, driven directly. An exponential draw below half a
  // nanosecond rounds to a zero gap, which would put an arrival on the start instant; the
  // one-nanosecond floor is the only thing that keeps it off. Deterministic and small, where the
  // break-at-the-start run it replaces needed ~100,000 records and still missed this case.
  it("never puts an arrival on the start instant, even when a gap rounds to zero", () => {
    const zero = Object.create(Rng.prototype) as Rng;
    zero.exponential = () => 0.4;
    const arrivals = new Arrivals(zero, () => 1e9, T0, T0 + 3n);
    expect([arrivals.next(), arrivals.next(), arrivals.next()]).toEqual([T0 + 1n, T0 + 2n, undefined]);
  });
});

// Astra review P2-1: the horizon renews with each arrival, so a recurring stream with no end keeps
// going. Discriminating: a horizon fixed at start + 400 days ended this stream on day 400.
describe("a stream with no end", () => {
  it("still yields arrivals past day 400 at a constant rate", () => {
    const rng = Object.create(Rng.prototype) as Rng;
    // A half-hour gap from each hour's start: one arrival an hour, at :30.
    rng.exponential = () => Number(HOUR) / 2;
    const arrivals = new Arrivals(rng, () => 2, T0);
    const day = 24n * HOUR;
    let last: bigint | undefined;
    for (let i = 0; i < 24 * 410; i++) last = arrivals.next();
    expect(last).toBeDefined();
    expect(last! - T0).toBeGreaterThan(405n * day);
  });
  it("ends when the rate stays zero for the whole idle horizon", () => {
    const rng = Object.create(Rng.prototype) as Rng;
    rng.exponential = () => Number(HOUR) / 2;
    const day = 24n * HOUR;
    // Positive for the first day only.
    const arrivals = new Arrivals(rng, (t) => (t < T0 + day ? 2 : 0), T0);
    let n = 0;
    while (arrivals.next() !== undefined) n++;
    expect(n).toBe(24);
  });
  // Astra round 2 P2-2: the idle horizon counts only zero-rate hours, so a stream whose rate is
  // positive in every hour never retires. Discriminating: this seed draws no arrival in its first
  // 400 days at 0.00001 an hour, and a horizon counted from the start returned undefined here.
  it("never retires while the rate is positive, however sparse", () => {
    const arrivals = new Arrivals(Rng.derive("sparse"), () => 0.00001, 0n);
    expect(arrivals.next()).toBeDefined();
  });
});

// Astra round 2 P2-3: one wait subscribes to its wake promise once, however many times it polls.
// Discriminating: a Promise.race per poll subscribed once per poll (26 for 25 polls).
describe("clockPacer", () => {
  it("subscribes to the wake promise once per wait, not once per poll", async () => {
    let thens = 0;
    class Counting<T> extends Promise<T> {
      override then<A = T, B = never>(
        onFulfilled?: ((value: T) => A | PromiseLike<A>) | null,
        onRejected?: ((reason: unknown) => B | PromiseLike<B>) | null,
      ): Promise<A | B> {
        thens++;
        return super.then(onFulfilled, onRejected);
      }
    }
    // Never settles: the wait ends only by reaching its target.
    const wake = new Counting<void>(() => {});
    let polls = 0;
    // One virtual nanosecond per read, so the wait polls until the 25th read.
    const clock: Clock = { nowNs: () => BigInt(polls++) };
    await clockPacer(clock, 1).waitUntil(25n, wake);
    expect(polls).toBeGreaterThanOrEqual(25);
    expect(thens).toBe(1);
  });
});

// Astra review P2-2 and P2-3.
describe("driver bounds", () => {
  const driver = (concurrency: number) => {
    const cfg = config();
    return new Driver({
      mix: parseMix(traffic()),
      config: cfg,
      makeEngine: makeEngineFor(runner, cfg),
      sink: new Recording(),
      pacer: manualPacer(T0),
      seed: "bounds",
      concurrency,
    });
  };
  it.each([0, -1, 1.5, Number.NaN, Number.POSITIVE_INFINITY])("refuses concurrency %s", (n) => {
    expect(() => driver(n)).toThrow(/concurrency must be a positive integer/);
  });
  it("removes each wait's abort listener when the wait ends", async () => {
    const controller = new AbortController();
    let added = 0;
    let removed = 0;
    const add = controller.signal.addEventListener.bind(controller.signal);
    const remove = controller.signal.removeEventListener.bind(controller.signal);
    controller.signal.addEventListener = ((...args: Parameters<typeof add>) => {
      added++;
      add(...args);
    }) as typeof add;
    controller.signal.removeEventListener = ((...args: Parameters<typeof remove>) => {
      removed++;
      remove(...args);
    }) as typeof remove;
    await driver(1).run({ until: T0 + 5n * MINUTE, signal: controller.signal });
    expect(added).toBeGreaterThan(10);
    expect(removed).toBe(added);
  }, 60_000);
});

describe("rates", () => {
  it("follow the daily and weekly curves over a virtual day", async () => {
    const daily = Array<number>(24).fill(1);
    daily[12] = 4;
    daily[3] = 0;
    const weekly = [1, 1, 2, 1, 1, 1, 1]; // Wednesday doubled; T0 is a Wednesday
    const m = requestsOnly() as { requests: { rate: unknown; name: string }[] };
    m.requests = m.requests.slice(0, 1);
    (m.requests[0] as { rate: unknown }).rate = { perHour: 300, daily, weekly };
    const { records } = await runDriver(runner, { mix: parseMix(m), duration: 24n * HOUR });
    const outcomes = records.filter((r) => payload(r)["eventName"] === "AM-ACCESS-OUTCOME");
    const perHour = Array<number>(24).fill(0);
    for (const r of outcomes) perHour[Number((ts(r) - T0) / HOUR)]! += 1;
    for (let h = 0; h < 24; h++) {
      const expected = 300 * 2 * daily[h]!;
      // Poisson: allow five standard deviations (and a floor for the empty hour).
      expect(Math.abs(perHour[h]! - expected), `hour ${h}: ${perHour[h]} against ${expected}`).toBeLessThanOrEqual(5 * Math.sqrt(expected) + 2);
    }
    expect(perHour[3]).toBeLessThanOrEqual(2); // the zero hour is quiet (stragglers from hour 2's elapsed time at most)
    const total = perHour.reduce((a, b) => a + b, 0);
    const wanted = 300 * 2 * daily.reduce((a, b) => a + b, 0);
    expect(Math.abs(total - wanted)).toBeLessThan(5 * Math.sqrt(wanted));
  });

  it("the scale option multiplies them", async () => {
    const mix = parseMix(requestsOnly(1000));
    const base = await runDriver(runner, { mix, duration: HOUR });
    const triple = await runDriver(runner, { mix, duration: HOUR, scale: 3 });
    const ratio = triple.report.requests / base.report.requests;
    expect(ratio).toBeGreaterThan(2.7);
    expect(ratio).toBeLessThan(3.3);
  });
});

/** Records not touched by a window: stamped before it, or well after it (elapsed times are under a minute). */
const outside = (records: readonly LogRecord[], from: bigint, to: bigint): LogRecord[] =>
  records.filter((r) => ts(r) < from || ts(r) >= to + MINUTE);

describe("breaks change records inside their window and nowhere else", () => {
  const from = T0 + HOUR;
  const to = from + 30n * MINUTE;
  const window = (kind: object) => parseBreaks(JSON.stringify([{ from: iso(from), to: iso(to), ...kind }]));
  const inside = (records: readonly LogRecord[]) => records.filter((r) => ts(r) >= from && ts(r) < to);
  const outcomes = (records: readonly LogRecord[], prefix: string) =>
    records.filter((r) => ["AM-ACCESS-OUTCOME"].includes(payload(r)["eventName"]) && String(payload(r)["http"]?.request?.path).includes(prefix));

  it("endpoint: requests under the path fail with the share, slower, and only then", async () => {
    const mix = parseMix(requestsOnly(1200));
    const base = await runDriver(runner, { mix, duration: 2n * HOUR });
    const broken = await runDriver(runner, { mix, duration: 2n * HOUR, breaks: window({ kind: "endpoint", path: "https://tenant.example.com/am/oauth2", share: 0.5, slower: 10 }) });
    const failed = (rs: readonly LogRecord[]) => outcomes(inside(rs), "/am/oauth2").filter((r) => payload(r)["response"].status === "FAILED");
    const all = outcomes(inside(broken.records), "/am/oauth2");
    const share = failed(broken.records).length / all.length;
    expect(all.length).toBeGreaterThan(100);
    expect(share).toBeGreaterThan(0.4); // the baseline's own failures are 5%
    expect(share).toBeLessThan(0.65);
    for (const r of failed(broken.records).filter((x) => !outcomes(inside(base.records), "/am/oauth2").some((y) => canon([y])[0] === canon([x])[0]))) {
      expect(payload(r)["response"].statusCode).toBe("500");
    }
    const medianMs = (rs: readonly LogRecord[]) => [...rs.map((r) => payload(r)["response"].elapsedTime as number)].sort((a, b) => a - b)[Math.floor(rs.length / 2)]!;
    expect(medianMs(failed(broken.records))).toBeGreaterThan(5 * medianMs(outcomes(inside(base.records), "/am/oauth2")));
    // Another path in the window is untouched, and nothing outside the window changed.
    expect(canon(outcomes(inside(broken.records), "/openidm"))).toEqual(canon(outcomes(inside(base.records), "/openidm")));
    expect(canon(outside(broken.records, from, to))).toEqual(canon(outside(base.records, from, to)));
  });

  it("node-fails: journeys at the node end there FAILED with the string \"false\"", async () => {
    const mix = parseMix({ ...(mixJson() as object), requests: [] });
    const base = await runDriver(runner, { mix, duration: 2n * HOUR });
    const broken = await runDriver(runner, { mix, duration: 2n * HOUR, breaks: window({ kind: "node-fails", node: "check", share: 1 }) });
    const checks = (rs: readonly LogRecord[]) =>
      inside(rs).filter((r) => payload(r)["eventName"] === "AM-NODE-LOGIN-COMPLETED" && payload(r)["entries"][0].info.nodeId === "check");
    expect(checks(broken.records).length).toBeGreaterThan(50);
    for (const r of checks(broken.records)) expect(payload(r)["entries"][0].info.nodeOutcome).toBe("false");
    // The baseline's check nodes all pass, so this separates the break from the tree's own failures.
    for (const r of checks(base.records)) expect(payload(r)["entries"][0].info.nodeOutcome).toBe("true");
    const trees = (rs: readonly LogRecord[]) => inside(rs).filter((r) => payload(r)["eventName"] === "AM-TREE-LOGIN-COMPLETED");
    expect(trees(broken.records).some((r) => payload(r)["result"] === "SUCCESSFUL")).toBe(false);
    expect(trees(base.records).some((r) => payload(r)["result"] === "SUCCESSFUL")).toBe(true);
    // A journey's tree record follows its check record under one trackingId.
    const failedTrackings = new Set(checks(broken.records).map((r) => payload(r)["trackingIds"][0]));
    const treeFails = trees(broken.records).filter((r) => failedTrackings.has(payload(r)["trackingIds"][0]));
    expect(treeFails.length).toBeGreaterThan(0);
    expect(treeFails.every((r) => payload(r)["result"] === "FAILED")).toBe(true);
    expect(canon(outside(broken.records, from, to))).toEqual(canon(outside(base.records, from, to)));
  }, 120_000);

  it("template: the new message appears at about the rate, only in the window", async () => {
    const mix = parseMix(requestsOnly());
    const base = await runDriver(runner, { mix, duration: 2n * HOUR });
    const broken = await runDriver(runner, {
      mix,
      duration: 2n * HOUR,
      breaks: window({ kind: "template", stream: "am-core", text: "cannot reach <*> for realm <*>", per_hour: 2000 }),
    });
    const lines = broken.records.filter((r) => r.source === "am-core");
    expect(base.records.some((r) => r.source === "am-core")).toBe(false);
    expect(Math.abs(lines.length - 1000)).toBeLessThan(5 * Math.sqrt(1000));
    expect(lines.every((r) => ts(r) >= from && ts(r) < to)).toBe(true);
    for (const r of lines) {
      expect(payload(r)["message"]).toMatch(/^cannot reach \S+ for realm \S+$/);
      expect(payload(r)["message"]).not.toContain("<*>");
      expect(payload(r)["level"]).toBe("ERROR");
      expect(payload(r)["logger"]).toMatch(/^org\.forgerock\./);
    }
    expect(canon(broken.records.filter((r) => r.source !== "am-core"))).toEqual(canon(base.records));
  });

  it("surge: extra requests from the network, only in the window", async () => {
    const mix = parseMix(requestsOnly());
    const base = await runDriver(runner, { mix, duration: 2n * HOUR });
    const broken = await runDriver(runner, { mix, duration: 2n * HOUR, breaks: window({ kind: "surge", network: "203.0.113.0/24", per_hour: 2000 }) });
    const fromNet = (rs: readonly LogRecord[]) => rs.filter((r) => String(payload(r)["client"]?.ip).startsWith("203.0.113."));
    expect(fromNet(base.records)).toHaveLength(0);
    const surged = fromNet(broken.records);
    // ~1000 requests; an am-access one makes two records, an idm-access one makes one.
    expect(surged.length).toBeGreaterThan(1000);
    expect(surged.length).toBeLessThan(2100);
    expect(surged.every((r) => ts(r) >= from && ts(r) < to + MINUTE)).toBe(true);
    expect(canon(broken.records.filter((r) => !fromNet([r]).length))).toEqual(canon(base.records));
  });

  it("two breaks at once do not disturb each other's draws", async () => {
    const mix = parseMix(requestsOnly());
    const t = { kind: "template", stream: "am-core", text: "x <*>", per_hour: 500 };
    const s = { kind: "surge", network: "203.0.113.0/24", per_hour: 500 };
    const only = await runDriver(runner, { mix, duration: 2n * HOUR, breaks: window(t) });
    const both = await runDriver(runner, {
      mix,
      duration: 2n * HOUR,
      breaks: parseBreaks(JSON.stringify([{ from: iso(from), to: iso(to), ...t }, { from: iso(from), to: iso(to), ...s }])),
    });
    expect(canon(both.records.filter((r) => r.source === "am-core"))).toEqual(canon(only.records.filter((r) => r.source === "am-core")));
  });
});

describe("records", () => {
  it("a journey's records share one tracking id across round trips, with string outcomes, tree last", async () => {
    const { records } = await runDriver(runner, { mix: parseMix({ ...(mixJson() as object), requests: [] }), duration: 10n * MINUTE });
    // An ATTEMPT carries no trackingIds (measured), so a journey is its tracking id plus
    // the records sharing a transaction id with one of its own.
    const txOf = new Map<string, Set<string>>();
    for (const r of records) {
      const id = payload(r)["trackingIds"]?.[0] as string | undefined;
      if (id !== undefined) txOf.set(id, (txOf.get(id) ?? new Set()).add(payload(r)["transactionId"]));
    }
    const done = [...txOf].filter(([id]) => records.some((r) => payload(r)["eventName"] === "AM-TREE-LOGIN-COMPLETED" && payload(r)["trackingIds"][0] === id));
    expect(done.length).toBeGreaterThan(20);
    for (const [id, txs] of done) {
      expect(txs.size).toBe(2); // two round trips: two transaction ids, one tracking id
      const rs = records.filter((r) => txs.has(payload(r)["transactionId"]));
      const names = [...rs].sort((a, b) => Number(ts(a) - ts(b))).map((r) => payload(r)["eventName"]).filter((n) => n !== undefined); // core lines carry none
      expect(names.filter((n) => n === "AM-ACCESS-ATTEMPT")).toHaveLength(2);
      const tree = names.indexOf("AM-TREE-LOGIN-COMPLETED");
      expect(names.slice(tree + 1)).toEqual(["AM-ACCESS-OUTCOME"]); // only the outcome follows the tree record
      for (const r of rs) {
        if (payload(r)["eventName"] === "AM-NODE-LOGIN-COMPLETED") {
          expect(payload(r)["trackingIds"][0]).toBe(id);
          expect(typeof payload(r)["entries"][0].info.nodeOutcome).toBe("string");
        }
      }
    }
  }, 60_000);

  it("uses synthetic people and the mix's networks only", async () => {
    const { records } = await runDriver(runner, { mix: parseMix(traffic()), duration: 10n * MINUTE });
    const text = JSON.stringify(records);
    expect(text).not.toMatch(/@(?!example\.com)[a-z]+\./);
    for (const r of records) {
      const ip = payload(r)["client"]?.ip;
      if (ip !== undefined) expect(ip).toMatch(/^198\.51\.100\.\d+$/);
    }
  });
});
