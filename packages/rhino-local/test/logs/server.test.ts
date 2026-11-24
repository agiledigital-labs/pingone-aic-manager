import { afterEach, describe, expect, it } from "vitest";
import { virtualClock, type Clock } from "../../src/logs/clock.ts";
import type { LogRecord, LogSource } from "../../src/logs/record.ts";
import { startLogsServer, type LogsServer, type LogsServerOptions } from "../../src/logs/server.ts";
import { DEFAULT_RETENTION_NS, LogStore } from "../../src/logs/store.ts";
import { formatRfc3339Ns, parseRfc3339Ns } from "../../src/logs/time.ts";

const KEY = { id: "key-id", secret: "key-secret" };
const T0 = parseRfc3339Ns("2026-11-18T00:00:00Z");
const SECOND = 1_000_000_000n;
const MICRO = 1_000n;

type Body = {
  result: LogRecord[];
  resultCount: number;
  pagedResultsCookie: string | null;
};

function record(source: LogSource, ns: bigint, extra: Record<string, unknown> = {}): LogRecord {
  return {
    source,
    timestamp: formatRfc3339Ns(ns),
    type: "application/json",
    payload: { eventName: "x", ...extra },
  };
}

type Rig = { server: LogsServer; store: LogStore; clock: { now: bigint }; real: { ms: number } };
const running: LogsServer[] = [];
afterEach(async () => {
  await Promise.all(running.splice(0).map((s) => s.close()));
});

async function rig(options: Partial<LogsServerOptions> & { retentionNs?: bigint } = {}): Promise<Rig> {
  const clock = { now: T0 + 10_000n * SECOND };
  const fake: Clock = { nowNs: () => clock.now };
  const real = { ms: 1_000_000 };
  const store = new LogStore({ clock: fake, ...(options.retentionNs === undefined ? {} : { retentionNs: options.retentionNs }) });
  const server = await startLogsServer({
    store,
    clock: fake,
    keys: KEY,
    requestsPerMinute: false,
    realNowMs: () => real.ms,
    ...options,
  });
  running.push(server);
  return { server, store, clock, real };
}

type Params = Record<string, string>;
async function get(r: Rig, params: Params, headers: Record<string, string> = { "x-api-key": KEY.id, "x-api-secret": KEY.secret }) {
  const url = new URL("/monitoring/logs", r.server.url);
  for (const [k, v] of Object.entries(params)) url.searchParams.set(k, v);
  const res = await fetch(url, { headers });
  return { status: res.status, headers: res.headers, body: (await res.json()) as Body & Record<string, unknown> };
}

const window = (extra: Params = {}): Params => ({
  source: "am-everything",
  beginTime: formatRfc3339Ns(T0),
  endTime: formatRfc3339Ns(T0 + 3600n * SECOND),
  ...extra,
});

describe("paging", () => {
  it("walks more than 1000 records in order, and a retried cookie returns the same page after new records arrive", async () => {
    const r = await rig();
    const total = 2500;
    // Distinct, strictly increasing timestamps 1 ms apart from T0+1s.
    for (let i = 0; i < total; i++) {
      r.store.emit(record("am-access", T0 + SECOND + BigInt(i) * 1_000_000n, { n: i }));
    }
    const first = await get(r, window());
    expect(first.status).toBe(200);
    expect(first.body.resultCount).toBe(1000);
    expect(first.body.result).toHaveLength(1000);
    const cookie1 = first.body.pagedResultsCookie;
    expect(cookie1).not.toBeNull();

    const second = await get(r, window({ _pagedResultsCookie: cookie1 as string }));
    // The cursor must move: a server that ignored it would repeat page one.
    expect((second.body.result[0]?.payload as { n: number }).n).toBe(1000);

    // Records arrive between the two fetches, with timestamps that fall inside
    // page two's range and before page one's end.
    r.store.emit(record("am-access", T0 + SECOND + 1_000_500n * 1_000n, { n: "late-in-page-2" }));
    r.store.emit(record("am-access", T0 + SECOND + 10n * 1_000_000n + 500n, { n: "late-in-page-1" }));
    r.store.emit(record("am-access", T0 + 3000n * SECOND, { n: "late-after" }));

    const retry = await get(r, window({ _pagedResultsCookie: cookie1 as string }));
    expect(retry.body.result).toEqual(second.body.result);
    expect(retry.body.pagedResultsCookie).toBe(second.body.pagedResultsCookie);

    // Control: the late records are really in the store, so a fresh window sees them.
    const fresh = await get(r, window());
    const names = fresh.body.result.map((x) => (x.payload as { n: unknown }).n);
    expect(names).toContain("late-in-page-1");

    // Walking the retried chain to the end yields exactly the original 2500, once each.
    const seen: unknown[] = first.body.result.map((x) => (x.payload as { n: unknown }).n);
    let cookie: string | null = cookie1;
    while (cookie !== null) {
      const page: Awaited<ReturnType<typeof get>> = await get(r, window({ _pagedResultsCookie: cookie }));
      seen.push(...page.body.result.map((x) => (x.payload as { n: unknown }).n));
      cookie = page.body.pagedResultsCookie;
    }
    expect(seen).toEqual(Array.from({ length: total }, (_, i) => i));
  });

  it("ends the window with a null cookie, and honours _pageSize", async () => {
    const r = await rig();
    for (let i = 0; i < 5; i++) r.store.emit(record("am-access", T0 + SECOND * BigInt(i + 1)));
    const a = await get(r, window({ _pageSize: "5" }));
    expect(a.body.resultCount).toBe(5);
    expect(a.body.pagedResultsCookie).toBeNull();
    const b = await get(r, window({ _pageSize: "2" }));
    expect(b.body.resultCount).toBe(2);
    expect(b.body.pagedResultsCookie).not.toBeNull();
    expect(Object.keys(b.body).sort()).toEqual(["pagedResultsCookie", "result", "resultCount"]);
  });

  it("orders by timestamp across sources with ties in arrival order", async () => {
    const r = await rig();
    r.store.emit(record("am-core", T0 + 2n * SECOND, { n: "b1" }));
    r.store.emit(record("am-access", T0 + 2n * SECOND, { n: "b2" }));
    r.store.emit(record("am-access", T0 + 1n * SECOND, { n: "a" }));
    const { body } = await get(r, window());
    expect(body.result.map((x) => (x.payload as { n: string }).n)).toEqual(["a", "b1", "b2"]);
  });

  it("refuses a cookie from a different query, and a malformed one", async () => {
    const r = await rig();
    for (let i = 0; i < 3; i++) r.store.emit(record("am-access", T0 + SECOND * BigInt(i + 1)));
    const a = await get(r, window({ _pageSize: "1" }));
    const cookie = a.body.pagedResultsCookie as string;
    expect((await get(r, window({ source: "idm-everything", _pagedResultsCookie: cookie }))).status).toBe(400);
    expect((await get(r, window({ _pagedResultsCookie: "not-a-cookie" }))).status).toBe(400);
  });

  it("rejects page sizes outside 1..1000", async () => {
    const r = await rig();
    for (const size of ["0", "1001", "abc", "-1"]) {
      expect((await get(r, window({ _pageSize: size }))).status, size).toBe(400);
    }
    expect((await get(r, window({ _pageSize: "1000" }))).status).toBe(200);
  });
});

describe("bounds are (beginTime, endTime], at nanosecond precision", () => {
  // One bound, three records around it: 1 us before, exactly on, 1 us after.
  const bound = T0 + 500n * MICRO;
  const around = async (r: Rig) => {
    r.store.emit(record("am-access", bound - MICRO, { n: "before" }));
    r.store.emit(record("am-access", bound, { n: "on" }));
    r.store.emit(record("am-access", bound + MICRO, { n: "after" }));
  };
  const names = (b: Body) => b.result.map((x) => (x.payload as { n: string }).n);

  it("beginTime is exclusive at an exact event timestamp", async () => {
    const r = await rig();
    await around(r);
    const { body } = await get(r, {
      source: "am-access",
      beginTime: formatRfc3339Ns(bound),
      endTime: formatRfc3339Ns(bound + 1000n * SECOND),
    });
    expect(names(body)).toEqual(["after"]);
  });

  it("endTime is inclusive at an exact event timestamp", async () => {
    const r = await rig();
    await around(r);
    const { body } = await get(r, {
      source: "am-access",
      beginTime: formatRfc3339Ns(bound - 1000n * SECOND),
      endTime: formatRfc3339Ns(bound),
    });
    expect(names(body)).toEqual(["before", "on"]);
  });

  it("adjacent windows sharing a boundary neither drop nor repeat the boundary event", async () => {
    const r = await rig();
    await around(r);
    const lo = await get(r, { source: "am-access", beginTime: formatRfc3339Ns(bound - 1000n * SECOND), endTime: formatRfc3339Ns(bound) });
    const hi = await get(r, { source: "am-access", beginTime: formatRfc3339Ns(bound), endTime: formatRfc3339Ns(bound + 1000n * SECOND) });
    expect([...names(lo.body), ...names(hi.body)]).toEqual(["before", "on", "after"]);
  });

  it("compares at 1 us, not at milliseconds", async () => {
    const r = await rig();
    await around(r);
    // A bound 1 us either side of "on" must separate the three records.
    const { body } = await get(r, {
      source: "am-access",
      beginTime: formatRfc3339Ns(bound - MICRO),
      endTime: formatRfc3339Ns(bound + MICRO - 1n),
    });
    expect(names(body)).toEqual(["on"]);
  });

  it("rejects a window wider than a day, naming the span, and accepts exactly a day", async () => {
    const r = await rig();
    const wide = await get(r, { source: "am-access", beginTime: "2026-11-01T00:00:00Z", endTime: "2026-11-14T21:00:00Z" });
    expect(wide.status).toBe(400);
    expect(String(wide.body["message"])).toMatch(/13\.88 days worth of data requested .* Cannot request more than one days worth of logs/);
    expect((await get(r, { source: "am-access", beginTime: "2026-11-01T00:00:00Z", endTime: "2026-11-02T00:00:00Z" })).status).toBe(200);
    expect((await get(r, { source: "am-access", beginTime: "2026-11-01T00:00:00Z", endTime: "2026-11-02T00:00:00.000000001Z" })).status).toBe(400);
  });

  it("requires endTime with beginTime, and a parsable time", async () => {
    const r = await rig();
    expect((await get(r, { source: "am-access", beginTime: "2026-11-01T00:00:00Z" })).status).toBe(400);
    expect((await get(r, { source: "am-access", beginTime: "x", endTime: "2026-11-01T00:00:00Z" })).status).toBe(400);
  });
});

describe("sources and transaction ids", () => {
  it("expands the aggregates to every am-* / idm-* stream and returns each record's own source", async () => {
    const r = await rig();
    const sources: LogSource[] = ["am-access", "am-core", "am-authentication", "idm-access", "idm-core", "idm-sync"];
    sources.forEach((s, i) => r.store.emit(record(s, T0 + SECOND * BigInt(i + 1))));
    const sourcesOf = async (source: string) => (await get(r, window({ source }))).body.result.map((x) => x.source);
    expect(await sourcesOf("am-everything")).toEqual(["am-access", "am-core", "am-authentication"]);
    expect(await sourcesOf("idm-everything")).toEqual(["idm-access", "idm-core", "idm-sync"]);
    expect(await sourcesOf("am-access,idm-core")).toEqual(["am-access", "idm-core"]);
    expect(await sourcesOf("am-everything,idm-everything")).toHaveLength(6);
    expect(await sourcesOf("am-access")).toEqual(["am-access"]);
  });

  it("refuses an unknown source and a missing one", async () => {
    const r = await rig();
    expect((await get(r, window({ source: "nope" }))).status).toBe(400);
    const { source: _s, ...noSource } = window();
    expect((await get(r, noSource)).status).toBe(400);
  });

  it("matches transactionId on a prefix, as the tenant does", async () => {
    const r = await rig();
    const ids = ["root-1/0/0", "root-1/0/1", "root-10/0/0", "other/0"];
    ids.forEach((id, i) => r.store.emit(record("am-access", T0 + SECOND * BigInt(i + 1), { transactionId: id })));
    r.store.emit({ source: "am-access", timestamp: formatRfc3339Ns(T0 + 9n * SECOND), type: "text/plain", payload: "root-1 raw text" });
    const idsOf = async (tx: string) =>
      (await get(r, window({ transactionId: tx }))).body.result.map((x) => (x.payload as { transactionId: string }).transactionId);
    expect(await idsOf("root-1")).toEqual(["root-1/0/0", "root-1/0/1", "root-10/0/0"]);
    expect(await idsOf("root-10")).toEqual(["root-10/0/0"]);
    expect(await idsOf("root-01")).toEqual([]);
    expect(await idsOf("root-1/0/1")).toEqual(["root-1/0/1"]);
  });

  it("paging with a transactionId filter keeps the filter across pages", async () => {
    const r = await rig();
    for (let i = 0; i < 6; i++) r.store.emit(record("am-access", T0 + SECOND * BigInt(i + 1), { transactionId: i % 2 === 0 ? "keep/0" : "drop/0" }));
    const a = await get(r, window({ transactionId: "keep", _pageSize: "2" }));
    const b = await get(r, window({ transactionId: "keep", _pageSize: "2", _pagedResultsCookie: a.body.pagedResultsCookie as string }));
    expect([...a.body.result, ...b.body.result]).toHaveLength(3);
    expect(b.body.pagedResultsCookie).toBeNull();
  });
});

describe("publication, retention and the clock", () => {
  it("hides a record until the clock reaches its timestamp", async () => {
    const r = await rig();
    const future = r.clock.now + 120n * SECOND;
    r.store.emit(record("am-access", future));
    const q = { source: "am-access", beginTime: formatRfc3339Ns(r.clock.now - 3000n * SECOND), endTime: formatRfc3339Ns(future + SECOND) };
    expect((await get(r, q)).body.resultCount).toBe(0);
    r.clock.now = future;
    expect((await get(r, q)).body.resultCount).toBe(1);
  });

  it("serves on a virtual clock: a record stamped at virtual now is visible the same instant", async () => {
    const real = { ns: parseRfc3339Ns("2026-11-22T09:04:00Z") };
    const clock = virtualClock(
      { realOriginNs: parseRfc3339Ns("2026-11-22T09:00:00Z"), virtualOriginNs: parseRfc3339Ns("2026-11-17T00:00:00Z"), rate: 15 },
      () => real.ns,
    );
    const store = new LogStore({ clock });
    const server = await startLogsServer({ store, clock, keys: KEY, requestsPerMinute: false });
    running.push(server);
    store.emit(record("am-access", clock.nowNs()));
    const url = new URL("/monitoring/logs", server.url);
    url.search = new URLSearchParams({ source: "am-access", beginTime: "2026-11-17T00:30:00Z", endTime: "2026-11-17T01:00:00Z" }).toString();
    const res = await fetch(url, { headers: { "x-api-key": KEY.id, "x-api-secret": KEY.secret } });
    expect(((await res.json()) as Body).resultCount).toBe(1);
  });

  it("drops records older than the retention, and keeps younger ones", async () => {
    const r = await rig({ retentionNs: 10n * 24n * 3600n * SECOND });
    const day = 24n * 3600n * SECOND;
    r.store.emit(record("am-access", r.clock.now - 20n * day));
    r.store.emit(record("am-access", r.clock.now - 9n * day));
    r.store.emit(record("am-access", r.clock.now - 1n * day));
    expect(r.store.count("am-access")).toBe(2);
    expect(DEFAULT_RETENTION_NS).toBe(30n * day);
  });
});

describe("authentication", () => {
  it("answers 401 without leaking records, for a missing, wrong or swapped key pair", async () => {
    const r = await rig();
    r.store.emit(record("am-access", T0 + SECOND));
    const cases: Record<string, string>[] = [
      {},
      { "x-api-key": KEY.id },
      { "x-api-key": KEY.id, "x-api-secret": "wrong" },
      { "x-api-key": "wrong", "x-api-secret": KEY.secret },
      { "x-api-key": KEY.secret, "x-api-secret": KEY.id },
      { authorization: "Bearer abc" },
    ];
    for (const headers of cases) {
      const res = await get(r, window(), headers);
      expect(res.status, JSON.stringify(headers)).toBe(401);
      expect(res.body.result).toBeUndefined();
    }
    expect((await get(r, window())).status).toBe(200);
  });
});

describe("rate limiting", () => {
  it("allows 15 requests per real minute per source, then 429 with whole-second Retry-After", async () => {
    const r = await rig({ requestsPerMinute: 15 });
    for (let i = 0; i < 15; i++) {
      expect((await get(r, window({ source: "am-everything" }))).status, `request ${i + 1}`).toBe(200);
    }
    r.real.ms += 15_000;
    const limited = await get(r, window({ source: "am-everything" }));
    expect(limited.status).toBe(429);
    expect(limited.headers.get("retry-after")).toBe("45");

    // A different source has its own quota: IDM's budget is not AM's.
    expect((await get(r, window({ source: "idm-everything" }))).status).toBe(200);

    // Rejected requests did not consume quota: waiting out the oldest hit admits again.
    r.real.ms += 45_000;
    expect((await get(r, window({ source: "am-everything" }))).status).toBe(200);
  });

  it("reports at least 1 second, rounded up", async () => {
    const r = await rig({ requestsPerMinute: 1 });
    await get(r, window());
    r.real.ms += 59_900;
    const limited = await get(r, window());
    expect(limited.status).toBe(429);
    expect(limited.headers.get("retry-after")).toBe("1");
  });

  it("can be disabled", async () => {
    const r = await rig({ requestsPerMinute: false });
    for (let i = 0; i < 40; i++) expect((await get(r, window())).status).toBe(200);
  });

  it("a 401 does not spend quota", async () => {
    const r = await rig({ requestsPerMinute: 2 });
    for (let i = 0; i < 5; i++) await get(r, window(), {});
    expect((await get(r, window())).status).toBe(200);
    expect((await get(r, window())).status).toBe(200);
    expect((await get(r, window())).status).toBe(429);
  });
});

describe("fault injection", () => {
  it("injects a 5xx, then lets the same request through", async () => {
    const r = await rig();
    r.store.emit(record("am-access", T0 + SECOND));
    r.server.setFault(({ attempt }) => (attempt === 1 ? { status: 503, retryAfterSeconds: 2 } : undefined));
    const first = await get(r, window({ source: "am-access" }));
    expect(first.status).toBe(503);
    expect(first.headers.get("retry-after")).toBe("2");
    expect(first.body.result).toBeUndefined();
    const second = await get(r, window({ source: "am-access" }));
    expect(second.status).toBe(200);
    expect(second.body.resultCount).toBe(1);
  });

  it("applies the option hook and can be cleared", async () => {
    const r = await rig({ fault: () => ({ status: 500 }) });
    expect((await get(r, window())).status).toBe(500);
    r.server.setFault(undefined);
    expect((await get(r, window())).status).toBe(200);
  });
});
