/**
 * The AIC log records one `authenticate` request writes, put into a `LogSink` in the order a
 * tenant writes them. Shapes are the ones docs/api/08-logs.md ("Script logger lines, and the
 * node completion record", verified 2026-11-25) and docs/api/25-journey-runtime.md ("What a
 * script can put in the audit log") measured on the sandbox. Where a field or an order is not
 * measured the comment says UNMEASURED, and the done-note lists each one.
 *
 * Nothing here knows any journey: the engine tells it what happened and it writes the records.
 */
import { randomUUID } from "node:crypto";
import type { LogRecord, LogSink } from "../logs/record.ts";
import type { JsonValue } from "../protocol.ts";
import type { AuthenticateRequest } from "./types.ts";

type Payload = Record<string, unknown>;
type Fields = { readonly [key: string]: unknown };

export type JourneyLogOptions = {
  readonly sink: LogSink;
  /** Epoch nanoseconds. Defaults to real time. */
  readonly now?: () => bigint;
  /** Used to spell `http.request.path` when the request carries no `url`. UNMEASURED fallback. */
  readonly origin?: string;
  /**
   * `response.elapsedTime` in milliseconds. Defaults to the emulator's own measured time between
   * the request's first and last record, which is zero under a clock that does not advance.
   */
  readonly latencyMs?: (info: { status: number; tree: string }) => number;
};

function realNow(): bigint {
  return BigInt(Math.round((performance.timeOrigin + performance.now()) * 1e6));
}

/** The payload's own `timestamp`: milliseconds, as the tenant writes it. */
export function isoMillis(ns: bigint): string {
  return new Date(Number(ns / 1_000_000n)).toISOString();
}

/** The envelope's `timestamp`: nanoseconds with trailing zeros trimmed, as measured (RFC 3339 nano). */
export function isoNanos(ns: bigint): string {
  const base = new Date(Number(ns / 1_000_000_000n) * 1000).toISOString().slice(0, 19);
  const fraction = (ns % 1_000_000_000n).toString().padStart(9, "0").replace(/0+$/, "");
  return fraction ? `${base}.${fraction}Z` : `${base}Z`;
}

/** Keys sorted at every depth, `undefined` as null: how the tenant wrote `auditInfo` (measured). */
function normaliseAudit(value: JsonValue): JsonValue {
  if (Array.isArray(value)) return value.map(normaliseAudit);
  if (value !== null && typeof value === "object")
    return Object.fromEntries(
      Object.keys(value)
        .sort()
        .map((key) => [key, normaliseAudit((value as { [k: string]: JsonValue })[key] ?? null)]),
    );
  return value;
}

const REASONS: Readonly<Record<number, string>> = {
  400: "Bad Request",
  401: "Unauthorized",
  500: "Internal Server Error",
};

/** What the engine knows about a script that threw. */
export type ScriptThrow = {
  readonly scriptName: string;
  /** Rhino's message for the throw, which already ends `(<script name>#<line>)`. */
  readonly thrown: string;
  readonly line: number;
  readonly column: number;
};

/** One emulator's counters; shared by its forks so ids stay unique. */
export class JourneyLogs {
  readonly sink: LogSink;
  readonly now: () => bigint;
  readonly origin: string;
  readonly latencyMs: JourneyLogOptions["latencyMs"];
  private readonly instance = randomUUID();
  private counter = 1_000_000;
  private requests = 0;

  constructor(options: JourneyLogOptions) {
    this.sink = options.sink;
    this.now = options.now ?? realNow;
    this.origin = options.origin ?? "https://tenant.example.com";
    this.latencyMs = options.latencyMs;
  }

  /** `<instance uuid>-<n>`: the shape of both `_id` and `trackingIds[0]` (measured). */
  nextId(): string {
    this.counter += 1;
    return `${this.instance}-${this.counter}`;
  }

  begin(request: AuthenticateRequest, realm: string): RequestLog {
    this.requests += 1;
    return new RequestLog(this, request, realm, this.requests, this.instance);
  }
}

export class RequestLog {
  /** The journey's identity: the same on every record of one journey, across round trips. */
  tracking: string | undefined;
  private readonly parent: JourneyLogs;
  private readonly request: AuthenticateRequest;
  private readonly realm: string;
  private readonly transactionId: string;
  private readonly thread: string;
  private readonly started: bigint;
  private scripts = 0;
  private readonly sequence: number;

  constructor(
    parent: JourneyLogs,
    request: AuthenticateRequest,
    realm: string,
    sequence: number,
    instance: string,
  ) {
    this.parent = parent;
    this.request = request;
    this.realm = realm;
    this.sequence = sequence;
    const supplied =
      request.transactionId ??
      Object.entries(request.headers ?? {}).find(
        ([name]) => name.toLowerCase() === "x-forgerock-transactionid",
      )?.[1][0];
    // `<id>/0` for a supplied id (measured). The generated spelling is UNMEASURED here: the
    // tenant's own is a `Root=` id (docs/api/08-logs.md), which no consumer parses.
    this.transactionId = `${supplied ?? `${instance}-request-${sequence}`}/0`;
    this.thread = `http-8080-${(sequence % 90) + 10}`;
    this.started = parent.now();
  }

  private write(source: LogRecord["source"], fields: Fields): void {
    const ns = this.parent.now();
    const payload: Payload = {
      ...fields,
      timestamp: isoMillis(ns),
      transactionId: this.transactionId,
    };
    this.parent.sink.emit({
      source,
      timestamp: isoNanos(ns),
      type: "application/json",
      payload: Object.fromEntries(Object.entries(payload).sort(([a], [b]) => (a < b ? -1 : 1))),
    });
  }

  private audit(
    source: "am-access" | "am-authentication",
    topic: "access" | "authentication",
    fields: Fields,
  ): void {
    this.write(source, {
      _id: this.parent.nextId(),
      component: "Authentication",
      level: "INFO",
      realm: `/${this.realm}`,
      source: "audit",
      topic,
      ...fields,
    });
  }

  private core(level: string, logger: string, message: string, extra: Fields = {}): void {
    this.write("am-core", {
      context: "default",
      level,
      logger,
      mdc: { transactionId: this.transactionId },
      message,
      thread: extra.thread ?? this.thread,
      ...Object.fromEntries(Object.entries(extra).filter(([key]) => key !== "thread")),
    });
  }

  private trackingId(): string {
    this.tracking ??= this.parent.nextId();
    return this.tracking;
  }

  private get ip(): string | undefined {
    return this.request.clientIp;
  }

  private httpRequest(): Payload {
    // UNMEASURED: whether AM logs a cookie or authorization header. They are left out.
    const headers: Record<string, readonly string[]> = {};
    for (const [name, values] of Object.entries(this.request.headers ?? {})) {
      const lower = name.toLowerCase();
      if (lower !== "cookie" && lower !== "authorization") headers[lower] = values;
    }
    if (this.request.userAgent && !headers["user-agent"])
      headers["user-agent"] = [this.request.userAgent];
    if (this.ip && !headers["x-real-ip"]) headers["x-real-ip"] = [this.ip];
    if (this.ip && !headers["x-forwarded-for"]) headers["x-forwarded-for"] = [this.ip];
    return {
      headers: Object.fromEntries(Object.entries(headers).sort(([a], [b]) => (a < b ? -1 : 1))),
      method: "POST",
      path:
        this.request.url ??
        `${this.parent.origin}/am/json/realms/root/realms/${this.realm}/authenticate`,
      queryParameters: Object.fromEntries(
        Object.entries(this.request.query).map(([key, value]) => [
          key,
          typeof value === "string" ? [value] : [...value],
        ]),
      ),
      secure: true,
    };
  }

  /** `AM-ACCESS-ATTEMPT`: first record of the request. It carries no `trackingIds` (measured). */
  attempt(): void {
    this.audit("am-access", "access", {
      ...(this.ip ? { client: { ip: this.ip } } : {}),
      eventName: "AM-ACCESS-ATTEMPT",
      http: { request: this.httpRequest() },
    });
  }

  /**
   * Script `logger.*` lines (`trace` is never written; `debug` is, on this sandbox). The logger
   * name is `scripts.<context>.<script id>.(<script name>)` with the decision-node context for
   * both engines (measured). Stamped when the script returns, not as each call ran: the runner
   * hands the lines back with the result.
   */
  scriptLines(
    lines: readonly { level: string; message: string }[],
    scriptId: string,
    scriptName: string,
  ): void {
    this.scripts += 1;
    for (const line of lines) {
      const level = line.level.toUpperCase();
      if (line.level === "trace" || !["DEBUG", "INFO", "WARN", "ERROR"].includes(level)) continue;
      this.core(
        level,
        `scripts.AUTHENTICATION_TREE_DECISION_NODE.${scriptId}.(${scriptName})`,
        line.message,
        // The pool's thread number is UNMEASURED beyond its shape (`ScriptEvaluator-<n>`).
        { thread: `ScriptEvaluator-${20 + ((this.sequence + this.scripts) % 10)}` },
      );
    }
  }

  /**
   * `AM-NODE-LOGIN-COMPLETED`: one per node that completed, exactly the measured `info` keys.
   * `nodeOutcome` is a string always: a consumer drops a boolean. `audit`, when given, is the
   * script's `auditEntryDetail` and goes under `nodeExtraLogging.auditInfo`.
   */
  nodeCompleted(node: {
    tree: string;
    nodeId: string;
    displayName: string;
    nodeType: string;
    outcome: string;
    authLevel: unknown;
    audit?: JsonValue;
  }): void {
    this.audit("am-authentication", "authentication", {
      entries: [
        {
          info: {
            authLevel: String(node.authLevel ?? 0),
            displayName: node.displayName,
            ...(node.audit === undefined
              ? {}
              : { nodeExtraLogging: { auditInfo: normaliseAudit(node.audit) } }),
            nodeId: node.nodeId,
            nodeOutcome: String(node.outcome),
            nodeType: node.nodeType,
            treeName: node.tree,
          },
        },
      ],
      eventName: "AM-NODE-LOGIN-COMPLETED",
      trackingIds: [this.trackingId()],
    });
  }

  /** A script threw: the measured am-core ERROR and three WARN lines. No node record follows. */
  scriptThrew(thrown: ScriptThrow): { exception: string; failureReason: string } {
    const core = `${thrown.thrown} in ${thrown.scriptName} at line number ${thrown.line} at column number ${thrown.column}`;
    const execution = `java.util.concurrent.ExecutionException: javax.script.ScriptException: ${core}`;
    const script = `javax.script.ScriptException: ${execution}`;
    const process = `org.forgerock.openam.auth.node.api.NodeProcessException: ${script}`;
    // The Java stack frames the tenant appends to `exception` are not reproduced.
    this.core(
      "ERROR",
      "org.forgerock.openam.scripting.ThreadPoolScriptEvaluator",
      `Script '${thrown.scriptName}' with evaluatorVersion 2.0 in realm /${this.realm} terminated with exception`,
      { exception: execution },
    );
    this.core(
      "WARN",
      "org.forgerock.openam.auth.nodes.ScriptedDecisionNode",
      "error evaluating the script",
      { exception: script },
    );
    this.core(
      "WARN",
      "org.forgerock.openam.auth.nodes.ScriptedDecisionNode",
      "An error occurred during scripted node processing",
      { exception: process },
    );
    this.core(
      "WARN",
      "org.forgerock.openam.core.rest.authn.trees.ExceptionProcessTreeResult",
      "Authentication tree failed with exception",
      { exception: process },
    );
    return {
      exception: "An exception occurred during the authentication process",
      failureReason: script,
    };
  }

  /** `auditEntryDetail` of an unacceptable type (an array, a number, a boolean): measured. */
  invalidAuditDetail(): { exception: string; failureReason: string } {
    const reason = "Invalid auditEntryDetail type from script";
    const exception = `org.forgerock.openam.auth.node.api.NodeProcessException: ${reason}`;
    this.core(
      "WARN",
      "org.forgerock.openam.auth.nodes.helpers.ScriptedNodeHelper",
      "script auditEntryDetail not type String or Map",
    );
    this.core(
      "WARN",
      "org.forgerock.openam.auth.nodes.ScriptedDecisionNode",
      "An error occurred during scripted node processing",
      { exception },
    );
    this.core(
      "WARN",
      "org.forgerock.openam.core.rest.authn.trees.ExceptionProcessTreeResult",
      "Authentication tree failed with exception",
      { exception },
    );
    return { exception: "An exception occurred during the authentication process", failureReason: reason };
  }

  /**
   * `AM-TREE-LOGIN-COMPLETED` with `result: "FAILED"`. `nodeExtraLogging` is `{failureReason:
   * "Login failure"}` when a node routed to Failure, and `{exception, failureReason}` when a
   * script threw (both measured). The two session-hook INFO lines that precede it on a failed
   * tree are measured too.
   */
  treeFailed(tree: string, authLevel: unknown, extra: Fields = { failureReason: "Login failure" }) {
    this.sessionHook();
    this.audit("am-authentication", "authentication", {
      entries: [
        {
          info: {
            authLevel: String(authLevel ?? 0),
            ...(this.ip ? { ipAddress: this.ip } : {}),
            nodeExtraLogging: extra,
            treeName: tree,
          },
        },
      ],
      eventName: "AM-TREE-LOGIN-COMPLETED",
      result: "FAILED",
      trackingIds: [this.trackingId()],
    });
  }

  private sessionHook(): void {
    const logger = "org.forgerock.openam.auth.nodes.framework.SessionHookFactory";
    this.core("INFO", logger, "creating session hook");
    this.core("INFO", logger, "session hook created successfully");
  }

  /**
   * `AM-TREE-LOGIN-COMPLETED` with `result: "SUCCESSFUL"`, as measured 2026-11-25 on the sandbox
   * twice (a journey that identified a user, and one through AnonymousUser): `entries[0].info` is
   * `{authLevel, ipAddress, treeName}` with no `nodeExtraLogging`; `principal` is `[<username>]`;
   * `userId` is `id=<universal id>,ou=user,o=<realm>,ou=services,ou=am-config`, the universal id
   * being the user's `_id` for an identified user and the anonymous user's name otherwise.
   * UNMEASURED: whether the session-hook lines precede a success as they precede a failure.
   */
  treeSucceeded(tree: string, authLevel: unknown, username: string, universalId?: string): void {
    this.sessionHook();
    this.audit("am-authentication", "authentication", {
      entries: [
        {
          info: {
            authLevel: String(authLevel ?? 0),
            ...(this.ip ? { ipAddress: this.ip } : {}),
            treeName: tree,
          },
        },
      ],
      eventName: "AM-TREE-LOGIN-COMPLETED",
      principal: [username],
      result: "SUCCESSFUL",
      trackingIds: [this.trackingId()],
      userId: `id=${universalId ?? username},ou=user,o=${this.realm},ou=services,ou=am-config`,
    });
  }

  /**
   * `AM-ACCESS-OUTCOME`, always last. A 401 is preceded by the measured am-core WARN. `statusCode`
   * is a string (measured). UNMEASURED: `detail.reason` for any status but 401, and the outcome
   * of a request that never reached a journey (a 400 has no `trackingIds` here).
   */
  outcome(status: number, tree: string): void {
    if (status === 401)
      this.core(
        "WARN",
        "org.forgerock.openam.core.rest.authn.http.AuthenticationServiceV2",
        "Authentication encountered an error: [Status: 401 Unauthorized]",
      );
    const measured = Number((this.parent.now() - this.started) / 1_000_000n);
    const elapsed = this.parent.latencyMs?.({ status, tree }) ?? measured;
    const ok = status >= 200 && status < 300;
    this.audit("am-access", "access", {
      ...(this.ip ? { client: { ip: this.ip } } : {}),
      eventName: "AM-ACCESS-OUTCOME",
      http: { request: this.httpRequest() },
      response: {
        ...(ok ? {} : { detail: { reason: REASONS[status] ?? String(status) } }),
        elapsedTime: elapsed,
        elapsedTimeUnits: "MILLISECONDS",
        status: ok ? "SUCCESSFUL" : "FAILED",
        statusCode: String(status),
      },
      ...(this.tracking ? { trackingIds: [this.trackingId()] } : {}),
    });
  }
}
