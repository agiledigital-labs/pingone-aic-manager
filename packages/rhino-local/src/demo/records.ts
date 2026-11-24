/**
 * Record builders: the envelope and payloads as docs/api/08-logs.md measured
 * them. Anything a tenant was not measured giving is listed in the README's
 * "Demo driver" section under "Unmeasured".
 */
import type { LogRecord, LogSource } from "../logs/record.ts";
import { formatRfc3339Ns } from "../logs/time.ts";

export type Payload = Record<string, unknown>;

/** Millisecond ISO timestamp, as the payload's own `timestamp` is written. */
export function isoMillis(ns: bigint): string {
  return new Date(Number(ns / 1_000_000n)).toISOString();
}

export function envelope(source: LogSource, ns: bigint, payload: Payload | string): LogRecord {
  return {
    source,
    timestamp: formatRfc3339Ns(ns),
    type: typeof payload === "string" ? "text/plain" : "application/json",
    payload,
  };
}

export type Where = {
  /** `https://host` or empty; the tenant records the URL as sent. */
  readonly origin: string;
  readonly path: string;
  readonly method: string;
  readonly query: Readonly<Record<string, string>>;
  readonly ip: string;
  readonly userAgent: string;
};

function httpRequest(w: Where): Payload {
  return {
    headers: {
      "user-agent": [w.userAgent],
      "x-forwarded-for": [w.ip],
      "x-real-ip": [w.ip],
      "x-forwarded-proto": ["https"],
    },
    method: w.method,
    path: `${w.origin}${w.path}`,
    queryParameters: Object.fromEntries(Object.entries(w.query).map(([k, v]) => [k, [v]])),
    secure: true,
  };
}

export type AmAccess = {
  readonly ns: bigint;
  readonly id: string;
  readonly tracking: string;
  readonly transactionId: string;
  readonly component: string;
  readonly realm: string;
  readonly where: Where;
  readonly userId?: string;
};

/** `AM-ACCESS-ATTEMPT`: no `response`. Measured shape (docs/api/08-logs.md, 2026-11-25). */
export function amAccessAttempt(a: AmAccess): LogRecord {
  return envelope("am-access", a.ns, {
    _id: a.id,
    client: { ip: a.where.ip },
    component: a.component,
    eventName: "AM-ACCESS-ATTEMPT",
    http: { request: httpRequest(a.where) },
    level: "INFO",
    realm: a.realm,
    source: "audit",
    timestamp: isoMillis(a.ns),
    topic: "access",
    trackingIds: [a.tracking],
    transactionId: a.transactionId,
    ...(a.userId === undefined ? {} : { userId: a.userId }),
  });
}

export type Response = {
  readonly status: "SUCCESSFUL" | "FAILED";
  readonly statusCode: number;
  readonly elapsedMs: number;
  readonly reason?: string;
};

/** `AM-ACCESS-OUTCOME`. `detail.reason` is measured on the 401 only; other reasons are not. */
export function amAccessOutcome(a: AmAccess, r: Response): LogRecord {
  return envelope("am-access", a.ns, {
    _id: a.id,
    client: { ip: a.where.ip },
    component: a.component,
    eventName: "AM-ACCESS-OUTCOME",
    http: { request: httpRequest(a.where) },
    level: "INFO",
    realm: a.realm,
    response: {
      ...(r.reason === undefined ? {} : { detail: { reason: r.reason } }),
      elapsedTime: Math.max(0, Math.round(r.elapsedMs)),
      elapsedTimeUnits: "MILLISECONDS",
      status: r.status,
      statusCode: String(r.statusCode),
    },
    source: "audit",
    timestamp: isoMillis(a.ns),
    topic: "access",
    trackingIds: [a.tracking],
    transactionId: a.transactionId,
    ...(a.userId === undefined ? {} : { userId: a.userId }),
  });
}

export type IdmAccess = {
  readonly ns: bigint;
  readonly id: string;
  readonly tracking: string;
  readonly transactionId: string;
  readonly where: Where;
  readonly userId: string;
  readonly roles: readonly string[];
};

/**
 * IDM's one `access` event, written when the request finishes. Measured:
 * `eventName: "access"`, no `component`, a bare-uuid `userId`, `roles`,
 * `response.{status,statusCode,elapsedTime}`. The `request.{protocol,operation}`
 * block is carried over from the measured AM CREST shape and is not itself
 * measured for IDM.
 */
export function idmAccess(a: IdmAccess, r: Response): LogRecord {
  return envelope("idm-access", a.ns, {
    _id: a.id,
    client: { ip: a.where.ip },
    eventName: "access",
    http: { request: httpRequest(a.where) },
    level: "INFO",
    request: { operation: a.where.method === "GET" ? "READ" : "ACTION", protocol: "CREST" },
    response: {
      elapsedTime: Math.max(0, Math.round(r.elapsedMs)),
      elapsedTimeUnits: "MILLISECONDS",
      status: r.status,
      statusCode: String(r.statusCode),
    },
    roles: a.roles,
    source: "audit",
    timestamp: isoMillis(a.ns),
    topic: "access",
    trackingIds: [a.tracking],
    transactionId: a.transactionId,
    userId: a.userId,
  });
}

export type CoreLine = {
  readonly source: "am-core" | "idm-core";
  readonly ns: bigint;
  readonly level: string | undefined;
  readonly logger: string | undefined;
  readonly message: string;
  readonly transactionId?: string;
  readonly thread: string;
};

/** A framework or script log line. Shape measured for `am-core` (docs/api/08-logs.md, "Script logger lines"). */
export function coreLine(c: CoreLine): LogRecord {
  return envelope(c.source, c.ns, {
    context: "default",
    ...(c.level === undefined ? {} : { level: c.level }),
    ...(c.logger === undefined ? {} : { logger: c.logger }),
    ...(c.transactionId === undefined ? {} : { mdc: { transactionId: c.transactionId } }),
    message: c.message,
    thread: c.thread,
    timestamp: isoMillis(c.ns),
    ...(c.transactionId === undefined ? {} : { transactionId: c.transactionId }),
  });
}
