/**
 * THIN ADAPTER, TO BE DELETED when feat/emulator-demo-logs-emit merges.
 *
 * That branch teaches `JourneyEmulator` to emit am-access, am-authentication
 * and am-core records itself, through `JourneyOptions.logSink` and
 * `JourneyOptions.now`, with an optional client IP and user agent on
 * `AuthenticateRequest`. Until it lands this file produces the same records
 * from outside:
 *
 * - am-access ATTEMPT and OUTCOME around each request, from what the driver
 *   knows of the request and the engine's reply;
 * - am-authentication `AM-NODE-LOGIN-COMPLETED`, by wrapping the engine's
 *   private `advance(state, outcome)`, which the engine calls exactly when a
 *   node completes with an outcome that is wired. That is the one reach into
 *   engine internals, and it is why this file is disposable;
 * - `AM-TREE-LOGIN-COMPLETED` at a terminal reply.
 *
 * Not produced here, because only the engine can: am-core script `logger`
 * lines, the throw-path am-core ERROR/WARN lines and `auditEntryDetail`.
 *
 * Merging means: delete this file, build the engine with `{ logSink, now,
 * http }` in `driver.ts`'s `makeEngine`, pass `clientIp` and `userAgent` on the
 * request, and give the engine one hook for what `forceFail` does below (a
 * break that makes a node's outcome `false` and ends the journey FAILED). The
 * driver's own logic does not change.
 *
 * One engine per journey: the engine's HttpMock and this tap then need no
 * context lookup, which an async-local store could not give them (the runner
 * calls the mock from its own read loop).
 */
import type { JourneyEmulator, JourneyOptions } from "../journey/engine.ts";
import type { AuthenticateRequest, AuthenticateResult, HttpMock } from "../journey/types.ts";
import { FAILURE_NODE_ID } from "../aic/constants.ts";
import type { JourneyConfig } from "../journey/config.ts";
import type { LogSink } from "../logs/record.ts";
import { envelope, amAccessAttempt, amAccessOutcome, isoMillis, type Where } from "./records.ts";

type Frame = { tree: string; node: string };
type AdvanceState = { frames: Frame[] };
type Tappable = { advance: (state: AdvanceState, outcome: string) => unknown };

export type SessionContext = {
  readonly tree: string;
  readonly config: JourneyConfig;
  readonly sink: LogSink;
  readonly realm: string;
  /** `https://host`, or empty. */
  readonly origin: string;
  readonly ip: string;
  readonly userAgent: string;
  /** The journey's `trackingIds[0]`: the same on every record of every round trip. */
  readonly tracking: string;
  readonly http: HttpMock;
  /** Counter for record ids, shared with the driver's other records of this journey. */
  readonly nextId: () => string;
  /** Whether a break fails the journey at `node` as it completes at time `t`. */
  readonly forceFail: (node: string, t: bigint) => boolean;
};

export type RequestTiming = {
  readonly t0: bigint;
  readonly elapsedNs: bigint;
  /** The caller's transaction id; records carry `<id>/0`. */
  readonly transactionId: string;
};

export type Reply = {
  readonly result: AuthenticateResult;
  /** A node completed with a break's outcome and the journey was ended there. */
  readonly forcedFailure: boolean;
};

export class JourneySession {
  readonly #ctx: SessionContext;
  readonly #engine: JourneyEmulator;
  #timing: RequestTiming | undefined;
  #nodes = 0;
  #forced = false;

  /** `make` builds an engine with the options given; the driver supplies the runner, IDM and config. */
  constructor(ctx: SessionContext, make: (options: JourneyOptions) => JourneyEmulator) {
    this.#ctx = ctx;
    this.#engine = make({ http: ctx.http });
    this.#tap();
  }

  #at(fraction: number): bigint {
    const timing = this.#timing as RequestTiming;
    return timing.t0 + BigInt(Math.floor(Number(timing.elapsedNs) * fraction));
  }

  #where(): Where {
    const c = this.#ctx;
    return {
      origin: c.origin,
      path: `/am/json/realms/root/realms/${c.realm}/authenticate`,
      method: "POST",
      query: { authIndexType: "service", authIndexValue: c.tree },
      ip: c.ip,
      userAgent: c.userAgent,
    };
  }

  #tap(): void {
    const engine = this.#engine as unknown as Tappable;
    const original = engine.advance.bind(engine);
    engine.advance = (state, outcome) => {
      const frame = state.frames.at(-1);
      const timing = this.#timing;
      if (frame === undefined || timing === undefined) return original(state, outcome);
      const tree = this.#ctx.config.tree(frame.tree);
      const node = tree?.nodes[frame.node];
      const at = this.#at(0.9 * (1 - 0.5 ** (this.#nodes + 1)));
      const fail = this.#ctx.forceFail(frame.node, at);
      const reply = original(state, outcome);
      if (reply !== undefined && !fail) return reply; // unwired outcome: no node record
      this.#nodes += 1;
      this.#ctx.sink.emit(
        envelope("am-authentication", at, {
          _id: this.#ctx.nextId(),
          component: "Authentication",
          entries: [
            {
              info: {
                authLevel: "0",
                displayName: node?.displayName ?? node?.nodeType ?? frame.node,
                nodeId: frame.node,
                // A string. Kikori drops a boolean.
                nodeOutcome: fail ? "false" : outcome,
                nodeType: node?.nodeType ?? "unknown",
                treeName: frame.tree,
              },
            },
          ],
          eventName: "AM-NODE-LOGIN-COMPLETED",
          level: "INFO",
          realm: `/${this.#ctx.realm}`,
          source: "audit",
          timestamp: isoMillis(at),
          topic: "authentication",
          trackingIds: [this.#ctx.tracking],
          transactionId: `${timing.transactionId}/0`,
        }),
      );
      if (fail) {
        // End the journey at the outermost tree's failure node, so the engine
        // itself produces the 401 and the terminal reply.
        state.frames.length = 1;
        const outer = state.frames[0];
        if (outer !== undefined) outer.node = FAILURE_NODE_ID;
        this.#forced = true;
        return undefined;
      }
      return reply;
    };
  }

  /** A request a break fails before the engine sees it: an access pair, FAILED, HTTP 500. */
  failRequest(timing: RequestTiming): void {
    const c = this.#ctx;
    this.#timing = timing;
    const tx = `${timing.transactionId}/0`;
    const access = (ns: bigint) => ({
      ns,
      id: c.nextId(),
      tracking: c.tracking,
      transactionId: tx,
      component: "Authentication",
      realm: `/${c.realm}`,
      where: this.#where(),
    });
    c.sink.emit(amAccessAttempt(access(timing.t0)));
    c.sink.emit(
      amAccessOutcome(access(timing.t0 + timing.elapsedNs), {
        status: "FAILED",
        statusCode: 500,
        elapsedMs: Number(timing.elapsedNs) / 1e6,
        reason: "Internal Server Error",
      }),
    );
  }

  /** One authenticate request: records around the engine's own work. */
  async request(timing: RequestTiming, body: AuthenticateRequest["body"]): Promise<Reply> {
    const c = this.#ctx;
    this.#timing = timing;
    this.#forced = false;
    const tx = `${timing.transactionId}/0`;
    const access = (ns: bigint) => ({
      ns,
      id: c.nextId(),
      tracking: c.tracking,
      transactionId: tx,
      component: "Authentication",
      realm: `/${c.realm}`,
      where: this.#where(),
    });
    c.sink.emit(amAccessAttempt(access(timing.t0)));
    const result = await this.#engine.request({
      method: "POST",
      path: `/am/json/realms/root/realms/${c.realm}/authenticate`,
      query: { authIndexType: "service", authIndexValue: c.tree },
      ...(body === undefined ? {} : { body }),
    });
    const sent = (result.body ?? {}) as Record<string, unknown>;
    const terminal = result.status === 401 || (result.status === 200 && typeof sent["tokenId"] === "string");
    if (terminal) this.#treeRecord(timing, result);
    const failed = result.status >= 400;
    c.sink.emit(
      amAccessOutcome(access(timing.t0 + timing.elapsedNs), {
        status: failed ? "FAILED" : "SUCCESSFUL",
        statusCode: result.status,
        elapsedMs: Number(timing.elapsedNs) / 1e6,
        ...(result.status === 401 ? { reason: "Unauthorized" } : {}),
      }),
    );
    return { result, forcedFailure: this.#forced };
  }

  /**
   * `AM-TREE-LOGIN-COMPLETED`. The FAILED shape is measured
   * (`result`, `info.{authLevel, ipAddress, treeName}` and
   * `nodeExtraLogging.failureReason`). The SUCCESSFUL one is NOT measured on
   * the sandbox: it takes the same envelope with `result: "SUCCESSFUL"` and no
   * `nodeExtraLogging`. One function, so a measurement changes one place.
   */
  #treeRecord(timing: RequestTiming, result: AuthenticateResult): void {
    const c = this.#ctx;
    const failed = result.status !== 200;
    const reason = (result.body as { message?: unknown } | null)?.message;
    c.sink.emit(
      envelope("am-authentication", this.#at(0.95), {
        _id: c.nextId(),
        component: "Authentication",
        entries: [
          {
            info: {
              authLevel: "0",
              ipAddress: c.ip,
              ...(failed ? { nodeExtraLogging: { failureReason: typeof reason === "string" ? reason : "Login failure" } } : {}),
              treeName: c.tree,
            },
          },
        ],
        eventName: "AM-TREE-LOGIN-COMPLETED",
        level: "INFO",
        realm: `/${c.realm}`,
        result: failed ? "FAILED" : "SUCCESSFUL",
        source: "audit",
        timestamp: isoMillis(this.#at(0.95)),
        topic: "authentication",
        trackingIds: [c.tracking],
        transactionId: `${timing.transactionId}/0`,
      }),
    );
  }
}
