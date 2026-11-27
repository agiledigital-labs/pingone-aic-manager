/**
 * In-memory log store: a `LogSink` producers emit into, and the query side the
 * monitoring API reads.
 *
 * Choices (say them here, once):
 * - Retention defaults to 30 days of *virtual* time, as a tenant keeps. Expired
 *   records are dropped lazily, so a record somewhat older than the retention
 *   may still be returned; none younger is ever dropped.
 * - Publication lag is zero: a record is visible to a query once virtual now >=
 *   its timestamp. Kikori's requirement is "published within two virtual
 *   minutes", which immediate visibility satisfies. A record stamped in the
 *   future stays hidden until the clock reaches it.
 * - Records are indexed per source, ordered by (timestamp, arrival sequence), so
 *   ties keep a stable order and a record arriving late with an older
 *   timestamp lands where its timestamp says.
 */
import type { Clock } from "./clock.ts";
import type { LogRecord, LogSink, LogSource } from "./record.ts";
import { parseRfc3339Ns } from "./time.ts";

export const DEFAULT_RETENTION_NS = 30n * 24n * 3600n * 1_000_000_000n;

/** Sources a tenant lists (docs/api/08-logs.md), besides the two aggregates. */
export const KNOWN_SOURCES: readonly string[] = [
  "am-access", "am-activity", "am-authentication", "am-config", "am-core",
  "ctsstore", "ctsstore-access", "ctsstore-config-audit", "ctsstore-upgrade",
  "idm-access", "idm-activity", "idm-authentication", "idm-config", "idm-core", "idm-sync",
  "userstore", "userstore-access", "userstore-config-audit", "userstore-ldif-importer", "userstore-upgrade",
];

export const AGGREGATES = ["am-everything", "idm-everything"] as const;

type Entry = { readonly ts: bigint; readonly seq: bigint; readonly record: LogRecord };

/** Where a page ended: the last record returned, as (timestamp, sequence). */
export type Position = { readonly ts: bigint; readonly seq: bigint };

/** A window snapshot: everything a page needs besides the filter. Cookies carry this. */
export type Snapshot = {
  /** Upper bound actually served: min(endTime, virtual now at the first page). */
  readonly effectiveEndNs: bigint;
  /** Records that arrived after this sequence are not part of the window. */
  readonly highSeq: bigint;
  readonly after: Position | null;
};

export type Query = {
  /** Concrete sources (aggregates already expanded). */
  readonly sources: readonly string[];
  /** Exclusive. */
  readonly beginNs: bigint;
  /** Inclusive. */
  readonly endNs: bigint;
  readonly transactionIdPrefix?: string | undefined;
  /** `_queryFilter=payload/transactionId eq "<id>"`: the whole id, not a prefix. */
  readonly transactionIdEquals?: string | undefined;
  readonly pageSize: number;
  /** Absent on the first page. */
  readonly snapshot?: Snapshot | undefined;
};

export type Page = {
  readonly records: readonly LogRecord[];
  /** Absent when the window is exhausted. */
  readonly next: Snapshot | null;
};

function transactionIdOf(record: LogRecord): string | undefined {
  const payload = record.payload;
  if (typeof payload === "string") {
    return undefined;
  }
  const id = payload["transactionId"];
  return typeof id === "string" ? id : undefined;
}

/** Index of the first entry whose ts >= `ts`. */
function lowerBound(entries: readonly Entry[], ts: bigint): number {
  let lo = 0;
  let hi = entries.length;
  while (lo < hi) {
    const mid = (lo + hi) >>> 1;
    if ((entries[mid] as Entry).ts < ts) lo = mid + 1;
    else hi = mid;
  }
  return lo;
}

/** Index of the first entry strictly after (ts, seq). */
function upperBound(entries: readonly Entry[], ts: bigint, seq: bigint): number {
  let lo = lowerBound(entries, ts);
  while (lo < entries.length) {
    const e = entries[lo] as Entry;
    if (e.ts > ts || e.seq > seq) break;
    lo++;
  }
  return lo;
}

export class LogStore implements LogSink {
  readonly #clock: Clock;
  readonly #retentionNs: bigint;
  readonly #bySource = new Map<string, Entry[]>();
  #seq = 0n;

  constructor(options: { clock: Clock; retentionNs?: bigint }) {
    this.#clock = options.clock;
    this.#retentionNs = options.retentionNs ?? DEFAULT_RETENTION_NS;
    if (this.#retentionNs <= 0n) {
      throw new Error("retention must be positive");
    }
  }

  get clock(): Clock {
    return this.#clock;
  }

  /** Highest sequence handed out so far. */
  get highSeq(): bigint {
    return this.#seq;
  }

  emit(record: LogRecord): void {
    const ts = parseRfc3339Ns(record.timestamp);
    const entries = this.#bySource.get(record.source) ?? [];
    this.#bySource.set(record.source, entries);
    const entry: Entry = { ts, seq: ++this.#seq, record };
    const last = entries[entries.length - 1];
    if (last === undefined || last.ts <= ts) {
      entries.push(entry);
    } else {
      entries.splice(upperBound(entries, ts, entry.seq), 0, entry);
    }
    this.#prune(entries);
  }

  #prune(entries: Entry[]): void {
    const cutoff = this.#clock.nowNs() - this.#retentionNs;
    const slack = this.#retentionNs / 100n;
    const first = entries[0];
    if (first === undefined || first.ts >= cutoff - slack) {
      return;
    }
    entries.splice(0, lowerBound(entries, cutoff));
  }

  /** Count of records held for a source (tests and diagnostics). */
  count(source: LogSource | string): number {
    return this.#bySource.get(source)?.length ?? 0;
  }

  /** One page of the window `query` describes. */
  page(query: Query): Page {
    const snapshot: Snapshot = query.snapshot ?? {
      effectiveEndNs: query.endNs < this.#clock.nowNs() ? query.endNs : this.#clock.nowNs(),
      highSeq: this.#seq,
      after: null,
    };
    const heads = query.sources.flatMap((source) => {
      const entries = this.#bySource.get(source);
      if (entries === undefined) return [];
      // Retention is lazy, so a source nobody writes to is pruned here too: otherwise an idle
      // source serves records past retention to fresh queries and cookie continuations alike.
      this.#prune(entries);
      const from =
        snapshot.after === null
          ? upperBoundTs(entries, query.beginNs)
          : upperBound(entries, snapshot.after.ts, snapshot.after.seq);
      return [{ entries, index: from }];
    });
    const out: Entry[] = [];
    const prefix = query.transactionIdPrefix;
    const equals = query.transactionIdEquals;
    let more = false;
    for (;;) {
      let best: (typeof heads)[number] | undefined;
      let bestEntry: Entry | undefined;
      for (const head of heads) {
        // Skip entries that are not part of this snapshot or filter.
        while (head.index < head.entries.length) {
          const e = head.entries[head.index] as Entry;
          if (e.ts > snapshot.effectiveEndNs) {
            head.index = head.entries.length;
            break;
          }
          const tx =
            prefix === undefined && equals === undefined ? undefined : transactionIdOf(e.record);
          if (
            e.seq > snapshot.highSeq ||
            (prefix !== undefined && !(tx?.startsWith(prefix) ?? false)) ||
            (equals !== undefined && tx !== equals)
          ) {
            head.index++;
            continue;
          }
          break;
        }
        const e = head.entries[head.index];
        if (e !== undefined && (bestEntry === undefined || e.ts < bestEntry.ts || (e.ts === bestEntry.ts && e.seq < bestEntry.seq))) {
          best = head;
          bestEntry = e;
        }
      }
      if (best === undefined || bestEntry === undefined) break;
      if (out.length === query.pageSize) {
        more = true;
        break;
      }
      out.push(bestEntry);
      best.index++;
    }
    const lastOut = out[out.length - 1];
    return {
      records: out.map((e) => e.record),
      next:
        more && lastOut !== undefined
          ? { ...snapshot, after: { ts: lastOut.ts, seq: lastOut.seq } }
          : null,
    };
  }
}

/** Index of the first entry with ts strictly greater than `ts`. */
function upperBoundTs(entries: readonly Entry[], ts: bigint): number {
  let lo = 0;
  let hi = entries.length;
  while (lo < hi) {
    const mid = (lo + hi) >>> 1;
    if ((entries[mid] as Entry).ts <= ts) lo = mid + 1;
    else hi = mid;
  }
  return lo;
}

/** Expands the aggregate names; unknown names are returned in `unknown`. */
export function expandSources(requested: readonly string[]): { sources: string[]; unknown: string[] } {
  const known = new Set<string>(KNOWN_SOURCES);
  const all = known;
  const out = new Set<string>();
  const unknown: string[] = [];
  for (const name of requested) {
    if (name === "am-everything" || name === "idm-everything") {
      const prefix = name === "am-everything" ? "am-" : "idm-";
      for (const s of all) if (s.startsWith(prefix)) out.add(s);
    } else if (known.has(name)) {
      out.add(name);
    } else {
      unknown.push(name);
    }
  }
  return { sources: [...out], unknown };
}
