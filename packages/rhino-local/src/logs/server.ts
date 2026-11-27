/**
 * The emulated `GET /monitoring/logs`, answering as docs/api/08-logs.md
 * measured the tenant (and as kikori-aic's client expects).
 *
 * Where this differs from Kikori's brief, the measured doc wins; the
 * differences are listed in the README's "Monitoring logs server" section.
 */
import { createServer, type IncomingMessage, type Server, type ServerResponse } from "node:http";
import type { AddressInfo } from "node:net";
import { timingSafeEqual } from "node:crypto";
import type { Clock } from "./clock.ts";
import { expandSources, type LogStore, type Snapshot } from "./store.ts";
import { formatRfc3339Ns, parseRfc3339Ns } from "./time.ts";

const MAX_PAGE = 1000;
const DAY_NS = 24n * 3600n * 1_000_000_000n;

/** What a fault hook sees: the request as the server understood it. */
export type FaultContext = {
  readonly sources: readonly string[];
  readonly cookie: string | null;
  readonly attempt: number;
};

/** A fault to inject instead of answering. */
export type Fault = {
  readonly status: number;
  readonly body?: unknown;
  readonly retryAfterSeconds?: number;
};

export type FaultHook = (request: FaultContext) => Fault | undefined;

export type LogsServerOptions = {
  readonly store: LogStore;
  /** The clock the store runs on (virtual or real); used for "now". */
  readonly clock: Clock;
  readonly keys: { readonly id: string; readonly secret: string };
  /** 0 picks an ephemeral port. */
  readonly port?: number;
  readonly host?: string;
  /** Requests per real minute for the whole environment, all sources together. `false` disables. Default 60, the tenant's documented limit. */
  readonly requestsPerMinute?: number | false;
  /** Real time in ms, for the quota window. Injectable for tests. */
  readonly realNowMs?: () => number;
  readonly fault?: FaultHook;
};

export type LogsServer = {
  readonly url: string;
  readonly port: number;
  setFault(hook: FaultHook | undefined): void;
  close(): Promise<void>;
};

class HttpError extends Error {
  readonly status: number;
  readonly headers: Record<string, string>;
  constructor(status: number, message: string, headers: Record<string, string> = {}) {
    super(message);
    this.status = status;
    this.headers = headers;
  }
}

const REASONS: Record<number, string> = {
  400: "Bad Request",
  401: "Unauthorized",
  404: "Not Found",
  405: "Method Not Allowed",
  429: "Too Many Requests",
  500: "Internal Server Error",
  501: "Not Implemented",
  502: "Bad Gateway",
  503: "Service Unavailable",
};

function sameSecret(given: string | undefined, expected: string): boolean {
  if (given === undefined) return false;
  const a = Buffer.from(given);
  const b = Buffer.from(expected);
  return a.length === b.length && timingSafeEqual(a, b);
}

function encodeCookie(query: CookieQuery, snapshot: Snapshot): string {
  if (snapshot.after === null) throw new Error("a cookie names a position");
  return Buffer.from(
    JSON.stringify({
      v: 2,
      q: query,
      e: snapshot.effectiveEndNs.toString(),
      h: snapshot.highSeq.toString(),
      t: snapshot.after.ts.toString(),
      s: snapshot.after.seq.toString(),
    }),
  ).toString("base64url");
}

/**
 * What a cookie belongs to. `beginGiven`/`endGiven` are the caller's explicit bounds (null when
 * omitted), which a continuation must repeat; `begin`/`end` are the window the first page resolved,
 * which a continuation reuses rather than re-resolving, so implicit bounds do not move with the
 * clock between pages (the emulator's own snapshot paging contract; tenant behaviour unmeasured).
 */
type CookieQuery = {
  sources: string;
  beginGiven: string | null;
  endGiven: string | null;
  begin: string;
  end: string;
  tx: string | null;
  eq: string | null;
};
/** The query parameters the tenant was seen to honour (docs/api/08-logs.md, "Query params"). */
const KNOWN_PARAMS = [
  "source",
  "beginTime",
  "endTime",
  "transactionId",
  "_queryFilter",
  "_pageSize",
  "_pagedResultsCookie",
];
/** A CREST string literal, read the way JSON reads one; undefined when it is not one. */
function literal(text: string | undefined): string | undefined {
  if (text === undefined) return undefined;
  try {
    const value: unknown = JSON.parse(text);
    return typeof value === "string" ? value : undefined;
  } catch {
    return undefined;
  }
}
const STRING = '("(?:[^"\\\\]|\\\\.)*")';
/** The one `_queryFilter` the tenant was seen to honour. */
const TX_FILTER = new RegExp(`^payload/transactionId eq ${STRING}$`);
/**
 * The filters doc 08 measured failing (2026-08-12, `am-core`, 20 s and 6 h windows alike):
 * `payload/logger eq "…"` and `payload/message co "…"` answer `500 Internal server error`. Only
 * the status is recorded; the body here is the emulator's CREST-style one. Measured standalone and
 * well formed, so a literal JSON cannot read, or either one beside a `transactionId`, is the 501.
 */
const MEASURED_500 = [
  new RegExp(`^payload/logger eq ${STRING}$`),
  new RegExp(`^payload/message co ${STRING}$`),
];
/**
 * The id a `_queryFilter` selects. doc 08 records `payload/transactionId eq "…"` working, and the
 * two `MEASURED_500` forms on `am-core` failing 500, which this answers too. Which other filters
 * fail is unrecorded, so every other filter, and those two on any other source, is refused as the
 * emulator's own 501, never answered unfiltered. Exact match is CREST `eq`; that the tenant does
 * not prefix-match here, as it does for the `transactionId` parameter, is unmeasured.
 */
function filteredTransaction(filter: string, sources: readonly string[]): string {
  const measured500 = MEASURED_500.map((r) => r.exec(filter)).find((m) => m !== null);
  if (
    measured500 &&
    literal(measured500[1]) !== undefined &&
    sources.length === 1 &&
    sources[0] === "am-core"
  )
    throw new HttpError(500, "Internal server error");
  const id = literal(TX_FILTER.exec(filter)?.[1]);
  if (id === undefined)
    throw new HttpError(
      501,
      `Not modelled by the emulator: _queryFilter ${filter} (only payload/transactionId eq "<id>" is)`,
    );
  return id;
}

function decodeCookie(
  cookie: string,
  expected: Omit<CookieQuery, "begin" | "end">,
): { snapshot: Snapshot; beginNs: bigint; endNs: bigint } {
  const bad = new HttpError(400, "Invalid _pagedResultsCookie");
  let value: unknown;
  try {
    value = JSON.parse(Buffer.from(cookie, "base64url").toString("utf8"));
  } catch {
    throw bad;
  }
  if (typeof value !== "object" || value === null) throw bad;
  const v = value as Record<string, unknown>;
  const q = v["q"] as Partial<CookieQuery> | undefined;
  if (
    v["v"] !== 2 ||
    q === undefined ||
    q.sources !== expected.sources ||
    q.beginGiven !== expected.beginGiven ||
    q.endGiven !== expected.endGiven ||
    q.tx !== expected.tx ||
    (q.eq ?? null) !== expected.eq
  ) {
    throw new HttpError(400, "_pagedResultsCookie does not belong to this query");
  }
  try {
    return {
      snapshot: {
        effectiveEndNs: BigInt(v["e"] as string),
        highSeq: BigInt(v["h"] as string),
        after: { ts: BigInt(v["t"] as string), seq: BigInt(v["s"] as string) },
      },
      beginNs: BigInt(q.begin as string),
      endNs: BigInt(q.end as string),
    };
  } catch {
    throw bad;
  }
}

/**
 * Sliding one-minute window for the whole server, i.e. one environment: the
 * tenant documents "60 requests/min per environment" (docs/api/08-logs.md,
 * from Ping's documentation; not measured on the sandbox). Sources share it.
 */
class Quota {
  #hits: number[] = [];
  readonly #perMinute: number;
  readonly #now: () => number;
  constructor(perMinute: number, now: () => number) {
    this.#perMinute = perMinute;
    this.#now = now;
  }

  /** Returns the seconds to wait, or 0 if admitted (and then charged). */
  charge(): number {
    const now = this.#now();
    this.#hits = this.#hits.filter((t) => t > now - 60_000);
    if (this.#hits.length >= this.#perMinute) {
      const oldest = this.#hits[0] as number;
      return Math.max(1, Math.ceil((oldest + 60_000 - now) / 1000));
    }
    this.#hits.push(now);
    return 0;
  }
}

export async function startLogsServer(options: LogsServerOptions): Promise<LogsServer> {
  const { store, clock, keys } = options;
  const perMinute = options.requestsPerMinute === undefined ? 60 : options.requestsPerMinute;
  const quota =
    perMinute === false ? undefined : new Quota(perMinute, options.realNowMs ?? (() => Date.now()));
  let fault = options.fault;
  const attempts = new Map<string, number>();

  const send = (res: ServerResponse, status: number, body: unknown, headers: Record<string, string> = {}): void => {
    const text = JSON.stringify(body);
    res.writeHead(status, { "content-type": "application/json", "content-length": Buffer.byteLength(text), ...headers });
    res.end(text);
  };
  const error = (res: ServerResponse, status: number, message: string, headers: Record<string, string> = {}): void => {
    send(res, status, { code: status, reason: REASONS[status] ?? "Error", message }, headers);
  };

  const handle = (req: IncomingMessage, res: ServerResponse): void => {
    const url = new URL(req.url ?? "/", "http://emulator.invalid");
    if (url.pathname !== "/monitoring/logs") {
      throw new HttpError(404, "Not Found");
    }
    if (req.method !== "GET") {
      throw new HttpError(405, "Method Not Allowed", { allow: "GET" });
    }
    const header = (name: string): string | undefined => {
      const v = req.headers[name];
      return Array.isArray(v) ? v[0] : v;
    };
    if (!sameSecret(header("x-api-key"), keys.id) || !sameSecret(header("x-api-secret"), keys.secret)) {
      throw new HttpError(401, "Unauthorized");
    }
    const param = (name: string): string | undefined => url.searchParams.get(name) ?? undefined;
    const unknownParams = [...new Set(url.searchParams.keys())].filter(
      (name) => !KNOWN_PARAMS.includes(name),
    );
    if (unknownParams.length > 0) {
      throw new HttpError(501, `Not modelled by the emulator: query parameter ${unknownParams.join(", ")}`);
    }
    // What the tenant does with a parameter given twice is unmeasured, and reading only the first
    // would answer a query nobody asked: refuse.
    const repeated = [...new Set(url.searchParams.keys())].filter(
      (name) => url.searchParams.getAll(name).length > 1,
    );
    if (repeated.length > 0) {
      throw new HttpError(501, `Not modelled by the emulator: query parameter given more than once: ${repeated.join(", ")}`);
    }

    const sourceParam = param("source");
    if (sourceParam === undefined || sourceParam === "") {
      throw new HttpError(400, "source is required");
    }
    const requested = sourceParam.split(",").map((s) => s.trim()).filter((s) => s !== "");
    const { sources, unknown } = expandSources(requested);
    if (unknown.length > 0) {
      throw new HttpError(400, `Unknown log source: ${unknown.join(", ")}`);
    }

    const cookie = param("_pagedResultsCookie") ?? null;
    const reportedAttempt = (attempts.get(`${sourceParam}|${cookie ?? ""}`) ?? 0) + 1;
    attempts.set(`${sourceParam}|${cookie ?? ""}`, reportedAttempt);

    if (quota !== undefined) {
      const wait = quota.charge();
      if (wait > 0) {
        throw new HttpError(429, "Too Many Requests", { "retry-after": String(wait) });
      }
    }
    const injected = fault?.({ sources: requested, cookie, attempt: reportedAttempt });
    if (injected !== undefined) {
      const headers: Record<string, string> =
        injected.retryAfterSeconds === undefined ? {} : { "retry-after": String(injected.retryAfterSeconds) };
      send(res, injected.status, injected.body ?? { code: injected.status, reason: REASONS[injected.status] ?? "Error", message: "injected fault" }, headers);
      return;
    }

    const pageSizeText = param("_pageSize");
    let pageSize = MAX_PAGE;
    if (pageSizeText !== undefined) {
      if (!/^\d+$/.test(pageSizeText) || Number(pageSizeText) < 1 || Number(pageSizeText) > MAX_PAGE) {
        throw new HttpError(400, `_pageSize must be an integer from 1 to ${MAX_PAGE}`);
      }
      pageSize = Number(pageSizeText);
    }

    const beginText = param("beginTime");
    const endText = param("endTime");
    if (beginText !== undefined && endText === undefined) {
      throw new HttpError(400, "endTime is required when beginTime is set");
    }
    const parse = (name: string, text: string): bigint => {
      try {
        return parseRfc3339Ns(text);
      } catch {
        throw new HttpError(400, `${name} must be an RFC 3339 timestamp`);
      }
    };
    const endGiven = endText === undefined ? null : parse("endTime", endText);
    const beginGiven = beginText === undefined ? null : parse("beginTime", beginText);
    // Unrecorded on the tenant: with neither bound, serve the last hour up to now.
    let endNs = endGiven ?? clock.nowNs();
    let beginNs = beginGiven ?? endNs - 3600n * 1_000_000_000n;
    if (endNs - beginNs > DAY_NS) {
      const days = (Number(endNs - beginNs) / Number(DAY_NS)).toFixed(2);
      throw new HttpError(
        400,
        `${days} days worth of data requested (${formatRfc3339Ns(beginNs)} to ${formatRfc3339Ns(endNs)}) Please limit the scope of your query to within a day: Cannot request more than one days worth of logs`,
      );
    }

    const transactionId = param("transactionId");
    const filterText = param("_queryFilter");
    // Before the filter is read, so the unmodelled combination is never answered as a measured 500.
    if (filterText !== undefined && transactionId !== undefined) {
      throw new HttpError(501, "Not modelled by the emulator: transactionId together with _queryFilter");
    }
    const equals = filterText === undefined ? undefined : filteredTransaction(filterText, requested);
    const asked = {
      sources: [...sources].sort().join(","),
      beginGiven: beginGiven === null ? null : beginGiven.toString(),
      endGiven: endGiven === null ? null : endGiven.toString(),
      tx: transactionId ?? null,
      eq: equals ?? null,
    };
    let snapshot: Snapshot | undefined;
    if (cookie !== null) {
      // A continuation pages the window its first page resolved, whatever the clock says now.
      ({ snapshot, beginNs, endNs } = decodeCookie(cookie, asked));
    }
    const cookieQuery: CookieQuery = { ...asked, begin: beginNs.toString(), end: endNs.toString() };
    const page = store.page({
      sources,
      beginNs,
      endNs,
      transactionIdPrefix: transactionId,
      transactionIdEquals: equals,
      pageSize,
      snapshot,
    });
    send(res, 200, {
      result: page.records,
      resultCount: page.records.length,
      pagedResultsCookie: page.next === null ? null : encodeCookie(cookieQuery, page.next),
    });
  };

  const server: Server = createServer((req, res) => {
    try {
      handle(req, res);
    } catch (caught) {
      if (caught instanceof HttpError) {
        error(res, caught.status, caught.message, caught.headers);
      } else {
        error(res, 500, caught instanceof Error ? caught.message : String(caught));
      }
    }
  });
  await new Promise<void>((resolve, reject) => {
    server.once("error", reject);
    server.listen(options.port ?? 0, options.host ?? "127.0.0.1", () => {
      server.off("error", reject);
      resolve();
    });
  });
  const { port } = server.address() as AddressInfo;
  const host = options.host ?? "127.0.0.1";
  return {
    url: `http://${host.includes(":") ? `[${host}]` : host}:${port}`,
    port,
    setFault(hook) {
      fault = hook;
    },
    close: () =>
      new Promise<void>((resolve, reject) => {
        server.closeAllConnections();
        server.close((e) => (e === undefined ? resolve() : reject(e)));
      }),
  };
}
