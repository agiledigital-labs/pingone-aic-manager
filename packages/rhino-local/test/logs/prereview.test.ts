/** Pre-review fixes (2026-11-25): the logs server refuses what it does not model, loudly. */
import { afterEach, describe, expect, it } from "vitest";
import { type Clock } from "../../src/logs/clock.ts";
import type { LogRecord, LogSource } from "../../src/logs/record.ts";
import { startLogsServer, type LogsServer, type LogsServerOptions } from "../../src/logs/server.ts";
import { LogStore } from "../../src/logs/store.ts";
import { formatRfc3339Ns, parseRfc3339Ns } from "../../src/logs/time.ts";

const KEY = { id: "key-id", secret: "key-secret" };
const T0 = parseRfc3339Ns("2026-11-18T00:00:00Z");
const SECOND = 1_000_000_000n;

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

async function rig(options: Partial<LogsServerOptions> & { retentionNs?: bigint; defaultQuota?: true } = {}): Promise<Rig> {
  const clock = { now: T0 + 10_000n * SECOND };
  const fake: Clock = { nowNs: () => clock.now };
  const real = { ms: 1_000_000 };
  const store = new LogStore({ clock: fake, ...(options.retentionNs === undefined ? {} : { retentionNs: options.retentionNs }) });
  const server = await startLogsServer({
    store,
    clock: fake,
    keys: KEY,
    ...(options.defaultQuota === true ? {} : { requestsPerMinute: false as const }),
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

describe("P2-4 _queryFilter and unknown parameters", () => {
  async function seeded() {
    const r = await rig();
    r.store.emit(record("am-access", T0 + SECOND, { transactionId: "keep" }));
    r.store.emit(record("am-access", T0 + 2n * SECOND, { transactionId: "keep/0" }));
    r.store.emit(record("am-access", T0 + 3n * SECOND, { transactionId: "drop" }));
    return r;
  }
  const ids = (body: Body) => body.result.map((r) => (r.payload as { transactionId: string }).transactionId);
  it('payload/transactionId eq "<id>" selects that id exactly', async () => {
    const res = await get(await seeded(), window({ _queryFilter: 'payload/transactionId eq "keep"' }));
    expect(res.status).toBe(200);
    // Discriminating: the transactionId parameter would also return keep/0, a prefix match.
    expect(ids(res.body)).toEqual(["keep"]);
  });
  it("pages a filtered query, and the cookie belongs to that filter", async () => {
    const r = await seeded();
    r.store.emit(record("am-access", T0 + 4n * SECOND, { transactionId: "keep" }));
    const filter = { _queryFilter: 'payload/transactionId eq "keep"', _pageSize: "1" };
    const first = await get(r, window(filter));
    expect(ids(first.body)).toEqual(["keep"]);
    const cookie = first.body.pagedResultsCookie;
    expect(cookie).not.toBeNull();
    const next = await get(r, window({ ...filter, _pagedResultsCookie: String(cookie) }));
    expect(ids(next.body)).toEqual(["keep"]);
    expect(next.body.pagedResultsCookie).toBeNull();
    const unfiltered = await get(r, window({ _pagedResultsCookie: String(cookie), _pageSize: "1" }));
    expect(unfiltered.status).toBe(400);
  });
  // Review round 10 P2-1: with both bounds omitted the first page resolves the last hour, and the
  // cookie carries that window. Discriminating: re-resolving it after the clock moves made the
  // continuation a different query, a 400, and the second match unreachable.
  it("pages a filtered query with implicit bounds while the clock advances", async () => {
    const r = await rig();
    const now = r.clock.now;
    r.store.emit(record("am-access", now - 20n * SECOND, { transactionId: "match" }));
    r.store.emit(record("am-access", now - 15n * SECOND, { transactionId: "other" }));
    r.store.emit(record("am-access", now - 10n * SECOND, { transactionId: "match" }));
    const query = { source: "am-access", _queryFilter: 'payload/transactionId eq "match"', _pageSize: "1" };
    const first = await get(r, query);
    expect(first.status).toBe(200);
    expect(first.body.result.map((x) => x.timestamp)).toEqual([formatRfc3339Ns(now - 20n * SECOND)]);
    r.clock.now += SECOND;
    const next = await get(r, { ...query, _pagedResultsCookie: String(first.body.pagedResultsCookie) });
    expect(next.status).toBe(200);
    expect(next.body.result.map((x) => x.timestamp)).toEqual([formatRfc3339Ns(now - 10n * SECOND)]);
    expect(next.body.pagedResultsCookie).toBeNull();
    // The cookie still belongs to its query: an explicit bound it was not issued with is a 400.
    const bounded = await get(r, {
      ...query,
      beginTime: formatRfc3339Ns(now - 3600n * SECOND),
      endTime: formatRfc3339Ns(now),
      _pagedResultsCookie: String(first.body.pagedResultsCookie),
    });
    expect(bounded.status).toBe(400);
  });
  // Review round 11 P2: retention applies on reads, not only when a record is emitted. Discriminating:
  // with no emit after the clock passes retention, a store pruning only on emit still served both.
  it("an idle source past retention serves nothing to a cookie or a fresh query", async () => {
    const r = await rig();
    const now = r.clock.now;
    r.store.emit(record("am-access", now - 20n * SECOND, { transactionId: "match" }));
    r.store.emit(record("am-access", now - 10n * SECOND, { transactionId: "match" }));
    const first = await get(r, { source: "am-access", _pageSize: "1" });
    expect(first.body.result).toHaveLength(1);
    r.clock.now += 31n * 86_400n * SECOND;
    const replay = await get(r, { source: "am-access", _pageSize: "1", _pagedResultsCookie: String(first.body.pagedResultsCookie) });
    expect(replay.status).toBe(200);
    expect(replay.body.result).toEqual([]);
    expect(replay.body.pagedResultsCookie).toBeNull();
    const fresh = await get(r, {
      source: "am-access",
      beginTime: formatRfc3339Ns(now - 3600n * SECOND),
      endTime: formatRfc3339Ns(now),
    });
    expect(fresh.status).toBe(200);
    expect(fresh.body.result).toEqual([]);
  });
  it.each([
    ['payload/eventName eq "x"'],
    ['payload/transactionId sw "keep"'],
    ['payload/transactionId eq "keep" and payload/eventName eq "x"'],
    ["true"],
  ])("refuses _queryFilter %s with a 501 naming it", async (filter) => {
    const res = await get(await seeded(), window({ _queryFilter: filter }));
    expect(res.status).toBe(501);
    expect(String(res.body["message"])).toContain(filter);
  });
  it("refuses an unknown parameter with a 501 naming it", async () => {
    const res = await get(await seeded(), window({ _fields: "payload" }));
    expect(res.status).toBe(501);
    expect(String(res.body["message"])).toContain("_fields");
  });
  it("refuses transactionId together with _queryFilter", async () => {
    const res = await get(
      await seeded(),
      window({ transactionId: "keep", _queryFilter: 'payload/transactionId eq "keep"' }),
    );
    expect(res.status).toBe(501);
  });
});

describe("review round 1: repeated parameters and the measured am-core 500s", () => {
  async function seeded() {
    const r = await rig();
    r.store.emit(record("am-access", T0 + SECOND, { transactionId: "keep" }));
    r.store.emit(record("am-access", T0 + 2n * SECOND, { transactionId: "drop" }));
    return r;
  }
  async function raw(r: Rig, query: [string, string][]) {
    const url = new URL("/monitoring/logs", r.server.url);
    for (const [k, v] of query) url.searchParams.append(k, v);
    const res = await fetch(url, { headers: { "x-api-key": KEY.id, "x-api-secret": KEY.secret } });
    return { status: res.status, body: (await res.json()) as Record<string, unknown> };
  }
  // P1-2. Discriminating: URLSearchParams.get reads only the first, which alone is a valid filter.
  it.each([
    ["_queryFilter", 'payload/transactionId eq "keep"', 'payload/logger eq "x"'],
    ["transactionId", "keep", "drop"],
    ["source", "am-access", "am-core"],
  ])("refuses %s given twice with a 501 naming it", async (name, first, second) => {
    const query = Object.entries(window()).filter(([k]) => k !== name);
    const res = await raw(await seeded(), [...query, [name, first], [name, second]]);
    expect(res.status).toBe(501);
    expect(String(res.body["message"])).toContain(name);
  });
  // P2-2: doc 08 measured these two on am-core as the tenant's 500.
  it.each([['payload/logger eq "x"'], ['payload/message co "x"']])(
    "answers %s on am-core with the measured 500",
    async (filter) => {
      const res = await get(await seeded(), window({ source: "am-core", _queryFilter: filter }));
      expect(res.status).toBe(500);
    },
  );
  it("keeps the 501 for the same filter on a source it was not measured on", async () => {
    const res = await get(await seeded(), window({ source: "am-access", _queryFilter: 'payload/logger eq "x"' }));
    expect(res.status).toBe(501);
  });
  // Review round 2 P2-2: the measured 500 is for the standalone, well-formed filter only.
  it("refuses a measured-500 filter beside a transactionId as the unmodelled combination", async () => {
    const res = await get(
      await seeded(),
      window({ source: "am-core", _queryFilter: 'payload/logger eq "x"', transactionId: "keep" }),
    );
    expect(res.status).toBe(501);
    expect(String(res.body["message"])).toContain("transactionId together with _queryFilter");
  });
  it.each([['payload/logger eq "\\q"'], ['payload/message co "\\q"']])(
    "refuses %s, a malformed literal, as unmodelled rather than the measured 500",
    async (filter) => {
      const res = await get(await seeded(), window({ source: "am-core", _queryFilter: filter }));
      expect(res.status).toBe(501);
      expect(String(res.body["message"])).toContain("_queryFilter");
    },
  );
});
