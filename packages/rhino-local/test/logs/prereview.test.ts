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
