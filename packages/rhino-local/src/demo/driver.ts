/**
 * The demo traffic driver: runs a traffic mix against a `JourneyEmulator`
 * (journeys executed for real, the engine's HttpMock answering as the mix says)
 * into a `LogSink`, on the shared clock.
 *
 * Scheduling is by virtual time. Each journey tree and each non-journey
 * request has its own Poisson arrival stream, with the rate of the virtual hour
 * it is in. An event runs when the clock reaches its time, up to `concurrency`
 * at once. When the clock is already past an event's time the driver is behind:
 * it runs the event at once, stamps it with its scheduled time (so the records
 * are what an on-time run would have written), and reports how far behind it
 * was and for which journeys.
 *
 * Determinism: every random draw comes from a stream keyed by the seed and the
 * thing it is for (a tree and journey number, a request kind and number, a
 * break), never from a shared sequence, and every record's timestamp comes from
 * the schedule, never from the wall clock. Same seed, same records, however
 * the real-time execution interleaves.
 */
import type { JourneyConfig } from "../journey/config.ts";
import type { JourneyEmulator, JourneyOptions } from "../journey/engine.ts";
import type { AuthenticateRequest, HttpMock } from "../journey/types.ts";
import type { Clock } from "../logs/clock.ts";
import type { LogRecord, LogSink } from "../logs/record.ts";
import { address } from "./cidr.ts";
import { endpointEffect, fill, inWindow, nodeFails, templateVoice, type Break } from "./breaks.ts";
import { person, ratePerHour, type JourneySpec, type Mix, type Network, type RequestSpec } from "./mix.ts";
import { Rng } from "./rng.ts";
import { amAccessAttempt, amAccessOutcome, coreLine, idmAccess, type Where } from "./records.ts";

const HOUR_NS = 3_600_000_000_000n;
const SECOND_NS = 1_000_000_000n;

/** Waits on the clock. */
export type Pacer = {
  nowNs(): bigint;
  /** Resolves once virtual time reaches `t`, or as soon as `wake` settles. */
  waitUntil(t: bigint, wake: Promise<unknown>): Promise<void>;
};

/** Real-time pacing against a (virtual) clock, polling at most every `maxSleepMs`. */
export function clockPacer(clock: Clock, maxSleepMs = 40): Pacer {
  let anchor: { virtual: bigint; real: number } | undefined;
  let rate = 1;
  return {
    nowNs: () => clock.nowNs(),
    async waitUntil(t, wake) {
      for (;;) {
        const now = clock.nowNs();
        if (now >= t) return;
        const real = performance.now();
        if (anchor === undefined) anchor = { virtual: now, real };
        else if (real - anchor.real > 200) rate = Math.max(1e-6, Number(now - anchor.virtual) / 1e6 / (real - anchor.real));
        const ms = Math.min(maxSleepMs, Math.max(1, Number(t - now) / 1e6 / rate));
        let timer: NodeJS.Timeout | undefined;
        const slept = new Promise<"slept">((resolve) => {
          timer = setTimeout(() => resolve("slept"), ms);
        });
        const woke = await Promise.race([slept, wake.then(() => "wake" as const)]);
        clearTimeout(timer);
        if (woke === "wake") return;
      }
    },
  };
}

/** A manual clock that jumps to whatever it is asked to wait for. For tests and flat-out runs. */
export function manualPacer(start: bigint): Pacer & { set(t: bigint): void } {
  let now = start;
  return {
    nowNs: () => now,
    set(t) {
      now = t;
    },
    waitUntil(t) {
      if (t > now) now = t;
      return Promise.resolve();
    },
  };
}

class Heap<T> {
  readonly #items: T[] = [];
  readonly #before: (a: T, b: T) => boolean;
  constructor(before: (a: T, b: T) => boolean) {
    this.#before = before;
  }
  get size(): number {
    return this.#items.length;
  }
  peek(): T | undefined {
    return this.#items[0];
  }
  push(item: T): void {
    const a = this.#items;
    a.push(item);
    let i = a.length - 1;
    while (i > 0) {
      const p = (i - 1) >> 1;
      if (!this.#before(a[i] as T, a[p] as T)) break;
      [a[i], a[p]] = [a[p] as T, a[i] as T];
      i = p;
    }
  }
  pop(): T | undefined {
    const a = this.#items;
    const top = a[0];
    const last = a.pop();
    if (a.length > 0 && last !== undefined) {
      a[0] = last;
      let i = 0;
      for (;;) {
        const l = 2 * i + 1;
        const r = l + 1;
        let m = i;
        if (l < a.length && this.#before(a[l] as T, a[m] as T)) m = l;
        if (r < a.length && this.#before(a[r] as T, a[m] as T)) m = r;
        if (m === i) break;
        [a[i], a[m]] = [a[m] as T, a[i] as T];
        i = m;
      }
    }
    return top;
  }
}

/** Poisson arrivals from a rate that is constant within each virtual hour. */
class Arrivals {
  #cursor: bigint;
  readonly #end: bigint | undefined;
  readonly #rng: Rng;
  readonly #rate: (t: bigint) => number;
  readonly #horizon: bigint;

  constructor(rng: Rng, rate: (t: bigint) => number, start: bigint, end?: bigint) {
    this.#rng = rng;
    this.#rate = rate;
    this.#cursor = start;
    this.#end = end;
    this.#horizon = start + 400n * 24n * HOUR_NS;
  }

  next(): bigint | undefined {
    for (;;) {
      if (this.#cursor >= this.#horizon || (this.#end !== undefined && this.#cursor >= this.#end)) return undefined;
      const perHour = this.#rate(this.#cursor);
      const hourEnd = (this.#cursor / HOUR_NS + 1n) * HOUR_NS;
      if (!(perHour > 0)) {
        this.#cursor = hourEnd;
        continue;
      }
      const gap = BigInt(Math.max(1, Math.round(this.#rng.exponential(perHour / Number(HOUR_NS)))));
      const at = this.#cursor + gap;
      if (at < hourEnd) {
        this.#cursor = at;
        return this.#end !== undefined && at >= this.#end ? undefined : at;
      }
      this.#cursor = hourEnd;
    }
  }
}

type Ev = {
  readonly t: bigint;
  /** Tie-break keys: stream, item, step. Unique, so order never depends on insertion order. */
  readonly k1: number;
  readonly k2: number;
  readonly k3: number;
  /** What the lag report calls it. */
  readonly tag: string;
  readonly run: () => void | Promise<void>;
};

const before = (a: Ev, b: Ev): boolean =>
  a.t !== b.t ? a.t < b.t : a.k1 !== b.k1 ? a.k1 < b.k1 : a.k2 !== b.k2 ? a.k2 < b.k2 : a.k3 < b.k3;

export type DriverOptions = {
  readonly mix: Mix;
  readonly config: JourneyConfig;
  /** Builds an engine with the options given. One is built per journey. */
  readonly makeEngine: (options: JourneyOptions) => JourneyEmulator;
  readonly sink: LogSink;
  readonly pacer: Pacer;
  readonly seed: string | number;
  readonly breaks?: readonly Break[];
  /** Multiplies every base rate (journeys and requests, not breaks). Default 1. */
  readonly scale?: number;
  /** Journeys in flight at once. Default 8. */
  readonly concurrency?: number;
  /** A journey that is still asking for callbacks after this many round trips stops. Default 12. */
  readonly maxRoundTrips?: number;
  /** Virtual lag past which the driver counts itself behind. Default 30 s. */
  readonly lagWarnNs?: bigint;
  readonly onBehind?: (info: { lagNs: bigint; tag: string }) => void;
  readonly onError?: (error: unknown, tag: string) => void;
};

export type LagReport = { maxNs: bigint; over: number; events: number };

export type Report = {
  fromNs: bigint;
  /** Virtual time the run reached. */
  toNs: bigint;
  realMs: number;
  journeys: {
    started: number;
    succeeded: number;
    failed: number;
    abandoned: number;
    /** Ended by an endpoint break's HTTP 500. */
    brokenByEndpoint: number;
    /** The engine answered 400, 500 or 501 (see `engineErrors`). */
    errored: number;
  };
  journeysByTree: Record<string, number>;
  requests: number;
  answers: Record<string, number>;
  records: { total: number; bySource: Record<string, number> };
  unmockedHttp: Record<string, number>;
  engineErrors: Record<string, number>;
  /** Overall and per journey tree or request name. */
  lag: { overall: LagReport; byTag: Record<string, LagReport> };
  /** True when any event ran later than `lagWarnNs` after its scheduled time. */
  behind: boolean;
  journeysPerRealSecond: number;
  recordsPerRealSecond: number;
};

class CountingSink implements LogSink {
  total = 0;
  readonly bySource: Record<string, number> = {};
  readonly #inner: LogSink;
  constructor(inner: LogSink) {
    this.#inner = inner;
  }
  emit(record: LogRecord): void {
    this.total += 1;
    this.bySource[record.source] = (this.bySource[record.source] ?? 0) + 1;
    this.#inner.emit(record);
  }
}

const ENGINE_ID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}-(\d+)$/;

/**
 * The engine numbers its record ids from a random instance uuid. Swap that
 * prefix for the journey's own, derived from the seed, so a seed fixes the
 * records. `_id`, `trackingIds` and a session record's `objectId` (which is
 * the tracking id) carry it.
 */
function stableIds(inner: LogSink, uuid: string): LogSink {
  const swap = (v: unknown): unknown => {
    const m = typeof v === "string" ? ENGINE_ID.exec(v) : null;
    return m === null ? v : `${uuid}-${m[1]}`;
  };
  return {
    emit(record) {
      const p = record.payload;
      if (typeof p === "string") return inner.emit(record);
      const out: Record<string, unknown> = { ...p };
      if ("_id" in out) out["_id"] = swap(out["_id"]);
      if ("objectId" in out) out["objectId"] = swap(out["objectId"]);
      if (Array.isArray(out["trackingIds"])) out["trackingIds"] = out["trackingIds"].map(swap);
      inner.emit({ ...record, payload: out });
    },
  };
}

function networkPick(networks: readonly Network[], rng: Rng): string {
  const n = networks[rng.weighted(networks.map((x) => x.share))] as Network;
  return address(n.cidr, rng);
}

type Journey = {
  readonly treeIdx: number;
  readonly idx: number;
  readonly spec: JourneySpec;
  readonly rng: Rng;
  readonly engine: JourneyEmulator;
  /** What the engine's `now`, `latencyMs` and `failAt` read: the request being run. */
  readonly run: { t0: bigint; k: number; elapsedMs: number };
  readonly uuid: string;
  readonly ip: string;
  readonly userAgent: string;
  /** Counter for the ids of records the driver itself writes (an endpoint-failed request). */
  failId: number;
  readonly user: { username: string; email: string; userId: string };
  /** Index of the callback response the journey stops at, or -1. */
  readonly abandonAt: number;
  authId: string | undefined;
  callbacks: unknown;
  round: number;
  /** Callback responses received so far. */
  received: number;
};

export class Driver {
  readonly #o: DriverOptions;
  readonly #sink: CountingSink;
  readonly #breaks: readonly Break[];
  readonly #scale: number;
  readonly #queue: Heap<Ev> = new Heap<Ev>(before);
  readonly #seed: string | number;
  readonly #realm: string;
  readonly #origin: string;
  readonly #trees: string[];
  readonly #inflight = new Set<Promise<void>>();
  #report: Report;
  #lag: { overall: LagReport; byTag: Map<string, LagReport> };
  #lastBehindCall = 0;

  constructor(options: DriverOptions) {
    this.#o = options;
    this.#seed = options.seed;
    this.#sink = new CountingSink(options.sink);
    this.#breaks = options.breaks ?? [];
    this.#scale = options.scale ?? 1;
    this.#realm = options.mix.tenant.realm ?? options.config.realm;
    this.#origin = options.mix.tenant.host === undefined ? "" : `https://${options.mix.tenant.host}`;
    this.#trees = Object.keys(options.mix.journeys);
    const missing = this.#trees.filter((t) => options.config.tree(t) === undefined);
    if (missing.length > 0) {
      throw new Error(`the mix names journeys the journeys file does not define: ${missing.map((m) => JSON.stringify(m)).join(", ")}`);
    }
    if (this.#realm !== options.config.realm) {
      throw new Error(`mix realm ${JSON.stringify(this.#realm)} differs from the journeys' realm ${JSON.stringify(options.config.realm)}`);
    }
    if (!(this.#scale > 0) || !Number.isFinite(this.#scale)) throw new Error("scale must be a positive number");
    this.#report = this.#blank(0n);
    this.#lag = { overall: { maxNs: 0n, over: 0, events: 0 }, byTag: new Map() };
  }

  #blank(from: bigint): Report {
    return {
      fromNs: from,
      toNs: from,
      realMs: 0,
      journeys: { started: 0, succeeded: 0, failed: 0, abandoned: 0, brokenByEndpoint: 0, errored: 0 },
      journeysByTree: {},
      requests: 0,
      answers: {},
      records: { total: 0, bySource: {} },
      unmockedHttp: {},
      engineErrors: {},
      lag: { overall: { maxNs: 0n, over: 0, events: 0 }, byTag: {} },
      behind: false,
      journeysPerRealSecond: 0,
      recordsPerRealSecond: 0,
    };
  }

  /** A snapshot of the counters so far. */
  report(): Report {
    const r = this.#report;
    return {
      ...r,
      records: { total: this.#sink.total, bySource: { ...this.#sink.bySource } },
      lag: { overall: { ...this.#lag.overall }, byTag: Object.fromEntries([...this.#lag.byTag].map(([k, v]) => [k, { ...v }])) },
      behind: this.#lag.overall.over > 0,
    };
  }

  /**
   * Runs from the pacer's now until virtual time `until` (forever when absent,
   * until `signal` aborts), then waits for events in flight.
   */
  async run(options: { until?: bigint; signal?: AbortSignal } = {}): Promise<Report> {
    const start = this.#o.pacer.nowNs();
    this.#report = this.#blank(start);
    this.#schedule(start, options.until);
    const real0 = performance.now();
    const concurrency = this.#o.concurrency ?? 8;
    const aborted = new Promise<void>((resolve) => options.signal?.addEventListener("abort", () => resolve(), { once: true }));
    while (options.signal?.aborted !== true) {
      const head = this.#queue.peek();
      if (head === undefined || (options.until !== undefined && head.t >= options.until)) {
        if (this.#inflight.size === 0) break;
        await Promise.race(this.#inflight);
        continue;
      }
      const now = this.#o.pacer.nowNs();
      if (head.t > now) {
        await this.#o.pacer.waitUntil(head.t, Promise.race([aborted, ...(this.#inflight.size > 0 ? [Promise.race(this.#inflight)] : [])]));
        continue;
      }
      if (this.#inflight.size >= concurrency) {
        await Promise.race([aborted, ...this.#inflight]);
        continue;
      }
      this.#queue.pop();
      this.#noteLag(now - head.t, head.tag);
      const task = Promise.resolve()
        .then(() => head.run())
        .catch((error: unknown) => {
          this.#o.onError?.(error, head.tag);
          this.#report.engineErrors[`task ${head.tag}: ${error instanceof Error ? error.message : String(error)}`.slice(0, 200)] =
            (this.#report.engineErrors[`task ${head.tag}: ${error instanceof Error ? error.message : String(error)}`.slice(0, 200)] ?? 0) + 1;
        })
        .finally(() => this.#inflight.delete(task));
      this.#inflight.add(task);
    }
    await Promise.all(this.#inflight);
    const out = this.report();
    out.toNs = options.until ?? this.#o.pacer.nowNs();
    out.realMs = performance.now() - real0;
    const seconds = Math.max(1e-9, out.realMs / 1000);
    out.journeysPerRealSecond = out.journeys.started / seconds;
    out.recordsPerRealSecond = out.records.total / seconds;
    return out;
  }

  #noteLag(lagNs: bigint, tag: string): void {
    const lag = lagNs > 0n ? lagNs : 0n;
    const warn = this.#o.lagWarnNs ?? 30n * SECOND_NS;
    for (const entry of [this.#lag.overall, this.#tagLag(tag)]) {
      entry.events += 1;
      if (lag > entry.maxNs) entry.maxNs = lag;
      if (lag > warn) entry.over += 1;
    }
    if (lag > warn && this.#o.onBehind !== undefined && performance.now() - this.#lastBehindCall > 10_000) {
      this.#lastBehindCall = performance.now();
      this.#o.onBehind({ lagNs: lag, tag });
    }
  }

  #tagLag(tag: string): LagReport {
    let entry = this.#lag.byTag.get(tag);
    if (entry === undefined) {
      entry = { maxNs: 0n, over: 0, events: 0 };
      this.#lag.byTag.set(tag, entry);
    }
    return entry;
  }

  #push(ev: Ev): void {
    this.#queue.push(ev);
  }

  /** Seeds every arrival stream. */
  #schedule(start: bigint, until: bigint | undefined): void {
    const mix = this.#o.mix;
    let stream = 0;
    for (const [treeIdx, tree] of this.#trees.entries()) {
      const spec = mix.journeys[tree] as JourneySpec;
      const k1 = stream++;
      const arrivals = new Arrivals(Rng.derive(this.#seed, "arrivals", "journey", tree), (t) => ratePerHour(spec.rate, t) * this.#scale, start, until);
      let n = 0;
      const arm = (): void => {
        const t = arrivals.next();
        if (t === undefined) return;
        const idx = n++;
        this.#push({
          t,
          k1,
          k2: idx,
          k3: 0,
          tag: tree,
          run: () => {
            arm();
            return this.#startJourney(treeIdx, spec, idx, t);
          },
        });
      };
      arm();
    }
    for (const [reqIdx, spec] of mix.requests.entries()) {
      const k1 = stream++;
      const arrivals = new Arrivals(Rng.derive(this.#seed, "arrivals", "request", spec.name), (t) => ratePerHour(spec.rate, t) * this.#scale, start, until);
      let n = 0;
      const arm = (): void => {
        const t = arrivals.next();
        if (t === undefined) return;
        const idx = n++;
        this.#push({
          t,
          k1,
          k2: idx,
          k3: 0,
          tag: spec.name,
          run: () => {
            arm();
            this.#request(reqIdx, spec, idx, t);
          },
        });
      };
      arm();
    }
    for (const b of this.#breaks) {
      if (b.kind !== "surge" && b.kind !== "template") continue;
      const k1 = stream++;
      const from = b.from > start ? b.from : start;
      const to = until !== undefined && until < b.to ? until : b.to;
      if (from >= to) continue;
      const arrivals = new Arrivals(Rng.derive(this.#seed, "arrivals", "break", b.index), () => b.perHour, from, to);
      let n = 0;
      const arm = (): void => {
        const t = arrivals.next();
        if (t === undefined) return;
        const idx = n++;
        this.#push({
          t,
          k1,
          k2: idx,
          k3: 0,
          tag: `break ${b.index} (${b.kind})`,
          run: () => {
            arm();
            if (b.kind === "surge") this.#surge(b, idx, t, b.network);
            else this.#template(b, idx, t);
          },
        });
      };
      arm();
    }
  }

  // ---- journeys ----------------------------------------------------------

  #startJourney(treeIdx: number, spec: JourneySpec, idx: number, t: bigint): Promise<void> {
    const rng = Rng.derive(this.#seed, "journey", spec.tree, idx);
    const httpRng = Rng.derive(this.#seed, "http", spec.tree, idx);
    const uuid = rng.uuid();
    const ip = networkPick(spec.networks, rng);
    const userAgent = rng.pick(spec.userAgents);
    const user = person(spec.users, rng.below(spec.users.count));
    const abandonAt = rng.unit() < spec.abandon && spec.steps.length > 0 ? rng.below(spec.steps.length) : -1;
    const key = `${spec.tree}.${idx}`;
    const http: HttpMock = (url) => {
      const rule = spec.http.find((r) => r.match.test(url));
      if (rule === undefined) {
        this.#report.unmockedHttp[url] = (this.#report.unmockedHttp[url] ?? 0) + 1;
        return { status: 404, body: null };
      }
      const r = rule.responses[httpRng.weighted(rule.responses.map((x) => x.share))];
      return r === undefined ? { status: 404, body: null } : { status: r.status, ...(r.body === undefined ? {} : { body: r.body as never }) };
    };
    const run = { t0: 0n, k: 0, elapsedMs: 0 };
    const engine = this.#o.makeEngine({
      http,
      logSink: stableIds(this.#sink, uuid),
      // Records are stamped from the schedule: the request's time, a hundred microseconds apart.
      now: () => run.t0 + BigInt(run.k++) * 100_000n,
      ...(this.#origin === "" ? {} : { origin: this.#origin }),
      latencyMs: () => run.elapsedMs,
      failAt: ({ nodeId }) => nodeFails(this.#breaks, this.#seed, run.t0, nodeId, key),
    });
    const j: Journey = { treeIdx, idx, spec, rng, engine, run, uuid, ip, userAgent, failId: 5_000_000, user, abandonAt, authId: undefined, callbacks: undefined, round: 0, received: 0 };
    this.#report.journeys.started += 1;
    this.#report.journeysByTree[spec.tree] = (this.#report.journeysByTree[spec.tree] ?? 0) + 1;
    return this.#journeyStep(j, t);
  }

  async #journeyStep(j: Journey, t: bigint): Promise<void> {
    const { spec, rng } = j;
    const key = `${spec.tree}.${j.idx}.${j.round}`;
    const path = `${this.#origin}/am/json/realms/root/realms/${this.#realm}/authenticate`;
    const baseMs = rng.lognormal(spec.latency.medianMs, spec.latency.p95Ms);
    const fault = endpointEffect(this.#breaks, this.#seed, t, path, key);
    const elapsedNs = BigInt(Math.max(1_000_000, Math.round(baseMs * (fault?.slower ?? 1) * 1e6)));
    const transactionId = rng.uuid();
    j.round += 1;
    if (fault !== undefined) {
      this.#failAuthenticate(j, t, elapsedNs, transactionId);
      this.#report.journeys.brokenByEndpoint += 1;
      return;
    }
    j.run.t0 = t;
    j.run.k = 0;
    j.run.elapsedMs = Number(elapsedNs) / 1e6;
    const request: AuthenticateRequest = {
      method: "POST",
      path: `/am/json/realms/root/realms/${this.#realm}/authenticate`,
      query: { authIndexType: "service", authIndexValue: spec.tree },
      ...(j.authId === undefined ? {} : { body: { authId: j.authId, callbacks: j.callbacks } as never }),
      url: path,
      clientIp: j.ip,
      userAgent: j.userAgent,
      transactionId,
    };
    const result = await j.engine.request(request);
    const reply = (result.body ?? {}) as { authId?: unknown; callbacks?: unknown; tokenId?: unknown };
    if (result.status === 200 && typeof reply.authId === "string" && Array.isArray(reply.callbacks)) {
      const at = j.received++;
      const step = spec.steps[Math.min(at, spec.steps.length - 1)];
      // Draw before deciding anything, so the draws do not depend on the decision.
      const answer = step === undefined ? undefined : step.answers[rng.weighted(step.answers.map((a) => a.share))];
      const think = step === undefined ? 0 : rng.between(step.thinkSeconds[0], step.thinkSeconds[1]);
      if (step === undefined || answer === undefined || j.abandonAt === at || j.round >= (this.#o.maxRoundTrips ?? 12)) {
        this.#report.journeys.abandoned += 1;
        return;
      }
      this.#report.answers[`${spec.tree}: ${answer.label} (${answer.outcome})`] =
        (this.#report.answers[`${spec.tree}: ${answer.label} (${answer.outcome})`] ?? 0) + 1;
      j.authId = reply.authId;
      j.callbacks = (reply.callbacks as { type?: string; input?: { name: string; value: unknown }[] }[]).map((cb) => {
        const copy = structuredClone(cb);
        const value = cb.type === undefined ? undefined : answer.fill[cb.type];
        const first = copy.input?.[0];
        if (value !== undefined && first !== undefined) {
          first.value = value.replaceAll("{username}", j.user.username).replaceAll("{email}", j.user.email).replaceAll("{userId}", j.user.userId);
        }
        return copy;
      });
      const next = t + elapsedNs + BigInt(Math.round(think * 1e9));
      this.#push({ t: next, k1: 1_000_000 + j.treeIdx, k2: j.idx, k3: j.round, tag: spec.tree, run: () => this.#journeyStep(j, next) });
      return;
    }
    if (result.status === 200) this.#report.journeys.succeeded += 1;
    else if (result.status === 401) this.#report.journeys.failed += 1;
    else {
      this.#report.journeys.errored += 1;
      const message = String((result.body as { message?: unknown } | null)?.message ?? result.status).slice(0, 160);
      this.#report.engineErrors[message] = (this.#report.engineErrors[message] ?? 0) + 1;
    }
  }

  /** A request an endpoint break fails before the engine sees it: an access pair, FAILED, HTTP 500. */
  #failAuthenticate(j: Journey, t: bigint, elapsedNs: bigint, transactionId: string): void {
    const where: Where = {
      origin: this.#origin,
      path: `/am/json/realms/root/realms/${this.#realm}/authenticate`,
      method: "POST",
      query: { authIndexType: "service", authIndexValue: j.spec.tree },
      ip: j.ip,
      userAgent: j.userAgent,
    };
    const a = (ns: bigint) => ({
      ns,
      id: `${j.uuid}-${j.failId++}`,
      tracking: `${j.uuid}-${j.failId}`,
      transactionId: `${transactionId}/0`,
      component: "Authentication",
      realm: `/${this.#realm}`,
      where,
    });
    this.#sink.emit(amAccessAttempt(a(t)));
    this.#sink.emit(
      amAccessOutcome(a(t + elapsedNs), { status: "FAILED", statusCode: 500, elapsedMs: Number(elapsedNs) / 1e6, reason: "Internal Server Error" }),
    );
  }

  // ---- non-journey requests ---------------------------------------------

  #request(reqIdx: number, spec: RequestSpec, k: number, t: bigint): void {
    const rng = Rng.derive(this.#seed, "request", spec.name, k);
    this.#report.requests += 1;
    this.#access(spec, `${reqIdx}.${k}`, rng, t, networkPick(spec.networks, rng));
  }

  /** The records of one non-journey request; `ip` is the caller's. */
  #access(spec: RequestSpec, key: string, rng: Rng, t: bigint, ip: string): void {
    const p = person(spec.users, rng.below(spec.users.count));
    const uuid = rng.uuid();
    let counter = 1_000_000 + rng.below(1_000_000);
    const nextId = (): string => `${uuid}-${counter++}`;
    const tracking = nextId();
    const transactionId = `${tracking}/0`;
    const status = spec.status[rng.weighted(spec.status.map((s) => s.share))] as RequestSpec["status"][number];
    const path = spec.path.replaceAll("{userId}", p.userId);
    const where: Where = { origin: this.#origin, path, method: spec.method, query: spec.query, ip, userAgent: rng.pick(spec.userAgents) };
    const baseMs = rng.lognormal(spec.latency.medianMs, spec.latency.p95Ms);
    const fault = endpointEffect(this.#breaks, this.#seed, t, `${this.#origin}${path}`, key);
    const elapsedMs = baseMs * (fault?.slower ?? 1);
    const elapsedNs = BigInt(Math.max(1_000_000, Math.round(elapsedMs * 1e6)));
    const response = {
      status: fault === undefined ? status.status : ("FAILED" as const),
      statusCode: fault === undefined ? status.statusCode : 500,
      elapsedMs,
    };
    if (spec.stream === "am-access") {
      const a = (ns: bigint) => ({
        ns,
        id: nextId(),
        tracking,
        transactionId,
        component: spec.component,
        realm: `/${this.#realm}`,
        where,
        userId: `id=${p.userId},ou=user,ou=am-config`,
      });
      this.#sink.emit(amAccessAttempt(a(t)));
      this.#sink.emit(amAccessOutcome(a(t + elapsedNs), response));
    } else {
      this.#sink.emit(
        idmAccess(
          { ns: t + elapsedNs, id: nextId(), tracking, transactionId, where, userId: p.userId, roles: ["internal/role/openidm-authorized"] },
          response,
        ),
      );
    }
    for (const line of spec.core) {
      if (rng.unit() >= line.chance) continue;
      this.#sink.emit(
        coreLine({
          source: line.stream,
          ns: t + BigInt(rng.below(Number(elapsedNs))),
          level: line.level,
          logger: line.logger,
          message: line.message,
          transactionId,
          thread: line.stream === "am-core" ? `http-8080-${1 + rng.below(40)}` : `http-nio-${1 + rng.below(40)}`,
        }),
      );
    }
  }

  // ---- breaks that make records -------------------------------------------

  /** One surge request: a request of a kind the hour already has, from the surge's network. */
  #surge(b: Break & { kind: "surge" }, k: number, t: bigint, network: { base: number; size: number; text: string }): void {
    const rng = Rng.derive(this.#seed, "surge", b.index, k);
    const mix = this.#o.mix;
    const kinds: { weight: number; spec: RequestSpec | JourneySpec }[] = [
      ...mix.requests.map((spec) => ({ weight: ratePerHour(spec.rate, t) * this.#scale, spec })),
      ...Object.values(mix.journeys).map((spec) => ({ weight: ratePerHour(spec.rate, t) * this.#scale, spec })),
    ];
    if (!kinds.some((x) => x.weight > 0)) return;
    const chosen = kinds[rng.weighted(kinds.map((x) => x.weight))]?.spec;
    if (chosen === undefined) return;
    const ip = address(network, rng);
    if ("name" in chosen) {
      this.#access(chosen, `surge.${b.index}.${k}`, rng, t, ip);
      return;
    }
    // A journey's request: the first authenticate POST, a 200 (a callback round trip).
    const uuid = rng.uuid();
    const tracking = `${uuid}-${1_000_000 + rng.below(1_000_000)}`;
    const elapsedMs = rng.lognormal(chosen.latency.medianMs, chosen.latency.p95Ms);
    const fault = endpointEffect(this.#breaks, this.#seed, t, `${this.#origin}/am/json/realms/root/realms/${this.#realm}/authenticate`, `surge.${b.index}.${k}`);
    const ms = elapsedMs * (fault?.slower ?? 1);
    const where: Where = {
      origin: this.#origin,
      path: `/am/json/realms/root/realms/${this.#realm}/authenticate`,
      method: "POST",
      query: { authIndexType: "service", authIndexValue: chosen.tree },
      ip,
      userAgent: rng.pick(chosen.userAgents),
    };
    let n = 0;
    const a = (ns: bigint) => ({
      ns,
      id: `${uuid}-${n++}`,
      tracking,
      transactionId: `${tracking}/0`,
      component: "Authentication",
      realm: `/${this.#realm}`,
      where,
    });
    this.#sink.emit(amAccessAttempt(a(t)));
    this.#sink.emit(
      amAccessOutcome(a(t + BigInt(Math.max(1_000_000, Math.round(ms * 1e6)))), {
        status: fault === undefined ? "SUCCESSFUL" : "FAILED",
        statusCode: fault === undefined ? 200 : 500,
        elapsedMs: ms,
      }),
    );
  }

  /** One record of a template break: a new message from a plausible logger. */
  #template(b: Break & { kind: "template" }, k: number, t: bigint): void {
    if (!inWindow(b, t)) return;
    const rng = Rng.derive(this.#seed, "template", b.index, k);
    const voice = templateVoice(b);
    const message = fill(b.text, rng);
    if (b.stream === "am-core" || b.stream === "idm-core") {
      this.#sink.emit(
        coreLine({
          source: b.stream,
          ns: t,
          level: voice.level,
          logger: voice.logger,
          message,
          transactionId: b.stream === "am-core" ? `${rng.uuid()}-${1_000_000 + rng.below(1_000_000)}/0` : undefined as never,
          thread: b.stream === "am-core" ? `http-8080-${1 + rng.below(40)}` : `http-nio-${1 + rng.below(40)}`,
        }),
      );
    } else {
      this.#sink.emit({ source: b.stream, timestamp: new Date(Number(t / 1_000_000n)).toISOString(), type: "application/json", payload: { message } });
    }
  }
}
