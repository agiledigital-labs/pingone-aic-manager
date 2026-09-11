import { deepEqual, matchesPattern } from "./equal.ts";
import { isPortable } from "./portable.ts";
import {
  ALLOW_UNDECLARED_CHANNELS,
  DEFAULT_ALLOW_UNDECLARED,
  LOG_LEVELS,
  OPENIDM_METHODS,
  OPENIDM_WRITE_METHODS,
} from "./types.ts";
import type {
  AllowUndeclared,
  AllowUndeclaredChannel,
  CallbackEffect,
  Case,
  Channel,
  Expect,
  HttpEffect,
  HttpExpect,
  JsonObject,
  LogEffect,
  LogExpect,
  LogLevel,
  Mismatch,
  OpenidmEffect,
  OpenidmExpect,
  OpenidmMethod,
  RecordedEffects,
  StateBucket,
  StateDiff,
  Verdict,
} from "./types.ts";
import { formatValue, isPlainObject, parseJsonObject, parseJsonValue } from "./util.ts";
import { validateCase } from "./validate.ts";

const EFFECTS_KEYS = [
  "outcome",
  "sharedState",
  "transientState",
  "secureState",
  "callbacks",
  "openidm",
  "http",
  "logs",
] as const;

const WRITE_METHODS: ReadonlySet<string> = new Set(OPENIDM_WRITE_METHODS);
const OPENIDM_METHOD_SET: ReadonlySet<string> = new Set(OPENIDM_METHODS);
const LOG_LEVEL_SET: ReadonlySet<string> = new Set(LOG_LEVELS);

type Strictness = { [K in AllowUndeclaredChannel]: boolean };

/**
 * Pure function: (case, recorded effects) → pass/fail with a readable
 * explanation of each difference. Does not run a script.
 */
export function judge(input: unknown, effects: unknown): Verdict {
  const kase = validateCase(input);
  const recorded = parseEffects(effects);
  const strictness = resolveStrictness(kase.expect.allowUndeclared);
  const mismatches: Mismatch[] = [
    ...judgeOutcome(kase, recorded),
    ...judgeState(
      "sharedState",
      kase.expect.sharedState,
      recorded.sharedState,
      strictness.sharedState
    ),
    ...judgeState(
      "transientState",
      kase.expect.transientState,
      recorded.transientState,
      strictness.transientState
    ),
    ...judgeState(
      "secureState",
      kase.expect.secureState,
      recorded.secureState,
      strictness.secureState
    ),
    ...judgeCallbacks(kase.expect, recorded, strictness.callbacks),
    ...judgeOpenidm(kase.expect, recorded, strictness),
    ...judgeHttp(kase.expect, recorded, strictness.http),
    ...judgeLogs(kase.expect, recorded, strictness.logs),
  ];
  return {
    pass: mismatches.length === 0,
    portable: isPortable(kase),
    mismatches,
    summary: mismatches.length === 0 ? "" : formatSummary(kase, mismatches),
  };
}

function resolveStrictness(flags: AllowUndeclared | undefined): Strictness {
  const resolved: Strictness = { ...DEFAULT_ALLOW_UNDECLARED };
  if (flags === undefined) {
    return resolved;
  }
  for (const key of ALLOW_UNDECLARED_CHANNELS) {
    const value = flags[key];
    if (value !== undefined) {
      resolved[key] = value;
    }
  }
  return resolved;
}

function judgeOutcome(kase: Case, effects: RecordedEffects): Mismatch[] {
  if (effects.outcome === kase.expect.outcome) {
    return [];
  }
  const actual =
    effects.outcome === null ? "<no outcome>" : formatValue(effects.outcome);
  const message =
    effects.outcome === null
      ? `outcome: expected ${formatValue(kase.expect.outcome)}, script produced no outcome`
      : `outcome: expected ${formatValue(kase.expect.outcome)}, actual ${actual}`;
  return [
    miss("outcome", "outcome", formatValue(kase.expect.outcome), actual, message),
  ];
}

function diffState(
  initial: JsonObject,
  final: JsonObject
): { added: JsonObject; changed: JsonObject; removed: string[] } {
  const added: JsonObject = {};
  const changed: JsonObject = {};
  const removed: string[] = [];
  const initialKeys = new Set(Object.keys(initial));
  const finalKeys = new Set(Object.keys(final));
  for (const key of finalKeys) {
    if (!initialKeys.has(key)) {
      const value = final[key];
      if (value !== undefined) {
        added[key] = value;
      }
      continue;
    }
    if (!deepEqual(initial[key], final[key])) {
      const value = final[key];
      if (value !== undefined) {
        changed[key] = value;
      }
    }
  }
  for (const key of initialKeys) {
    if (!finalKeys.has(key)) {
      removed.push(key);
    }
  }
  removed.sort();
  return { added, changed, removed };
}

function judgeState(
  channel: "sharedState" | "transientState" | "secureState",
  expected: StateDiff | undefined,
  bucket: StateBucket,
  allowUndeclared: boolean
): Mismatch[] {
  const actual = diffState(bucket.initial, bucket.final);
  const expectedAdded = expected?.added ?? {};
  const expectedChanged = expected?.changed ?? {};
  const expectedRemoved = expected?.removed ?? [];
  const expectedRemovedSet = new Set(expectedRemoved);
  const mismatches: Mismatch[] = [];

  for (const key of Object.keys(expectedAdded)) {
    const want = expectedAdded[key];
    if (Object.prototype.hasOwnProperty.call(actual.added, key)) {
      if (!deepEqual(want, actual.added[key])) {
        mismatches.push(
          miss(
            channel,
            key,
            formatValue(want),
            formatValue(actual.added[key]),
            `${channel}: added ${formatValue(key)} differed`
          )
        );
      }
      continue;
    }
    if (Object.prototype.hasOwnProperty.call(actual.changed, key)) {
      mismatches.push(
        miss(
          channel,
          key,
          `added ${formatValue(want)}`,
          `changed to ${formatValue(actual.changed[key])}`,
          `${channel}: ${formatValue(key)} was changed, not added (already present in initial state)`
        )
      );
      continue;
    }
    if (actual.removed.includes(key)) {
      mismatches.push(
        miss(
          channel,
          key,
          `added ${formatValue(want)}`,
          "removed",
          `${channel}: ${formatValue(key)} was removed, not added`
        )
      );
      continue;
    }
    if (Object.prototype.hasOwnProperty.call(bucket.final, key)) {
      mismatches.push(
        miss(
          channel,
          key,
          `added ${formatValue(want)}`,
          `unchanged ${formatValue(bucket.final[key])}`,
          `${channel}: ${formatValue(key)} was unchanged, not added`
        )
      );
      continue;
    }
    mismatches.push(
      miss(
        channel,
        key,
        formatValue(want),
        "<absent>",
        `${channel}: missing added ${formatValue(key)}`
      )
    );
  }

  for (const key of Object.keys(expectedChanged)) {
    const want = expectedChanged[key];
    if (Object.prototype.hasOwnProperty.call(actual.changed, key)) {
      if (!deepEqual(want, actual.changed[key])) {
        mismatches.push(
          miss(
            channel,
            key,
            formatValue(want),
            formatValue(actual.changed[key]),
            `${channel}: changed ${formatValue(key)} differed`
          )
        );
      }
      continue;
    }
    if (Object.prototype.hasOwnProperty.call(actual.added, key)) {
      mismatches.push(
        miss(
          channel,
          key,
          `changed to ${formatValue(want)}`,
          `added ${formatValue(actual.added[key])}`,
          `${channel}: ${formatValue(key)} was added, not changed (absent from initial state)`
        )
      );
      continue;
    }
    if (actual.removed.includes(key)) {
      mismatches.push(
        miss(
          channel,
          key,
          `changed to ${formatValue(want)}`,
          "removed",
          `${channel}: ${formatValue(key)} was removed, not changed`
        )
      );
      continue;
    }
    if (Object.prototype.hasOwnProperty.call(bucket.final, key)) {
      mismatches.push(
        miss(
          channel,
          key,
          `changed to ${formatValue(want)}`,
          `unchanged ${formatValue(bucket.final[key])}`,
          `${channel}: ${formatValue(key)} did not change`
        )
      );
      continue;
    }
    mismatches.push(
      miss(
        channel,
        key,
        formatValue(want),
        "<absent>",
        `${channel}: missing changed ${formatValue(key)}`
      )
    );
  }

  for (const key of expectedRemoved) {
    if (actual.removed.includes(key)) {
      continue;
    }
    if (Object.prototype.hasOwnProperty.call(bucket.final, key)) {
      mismatches.push(
        miss(
          channel,
          key,
          "removed",
          formatValue(bucket.final[key]),
          `${channel}: ${formatValue(key)} was not removed`
        )
      );
      continue;
    }
    mismatches.push(
      miss(
        channel,
        key,
        "removed",
        "<absent from initial state>",
        `${channel}: ${formatValue(key)} was absent, not removed`
      )
    );
  }

  if (allowUndeclared) {
    return mismatches;
  }

  const mentioned = (key: string): boolean =>
    Object.prototype.hasOwnProperty.call(expectedAdded, key) ||
    Object.prototype.hasOwnProperty.call(expectedChanged, key) ||
    expectedRemovedSet.has(key);

  for (const key of Object.keys(actual.added)) {
    if (mentioned(key)) {
      continue;
    }
    mismatches.push(
      miss(
        channel,
        key,
        "(none)",
        formatValue(actual.added[key]),
        `${channel}: undeclared added ${formatValue(key)}`
      )
    );
  }
  for (const key of Object.keys(actual.changed)) {
    if (mentioned(key)) {
      continue;
    }
    mismatches.push(
      miss(
        channel,
        key,
        "(none)",
        formatValue(actual.changed[key]),
        `${channel}: undeclared changed ${formatValue(key)}`
      )
    );
  }
  for (const key of actual.removed) {
    if (mentioned(key)) {
      continue;
    }
    mismatches.push(
      miss(
        channel,
        key,
        "(none)",
        "removed",
        `${channel}: undeclared removed ${formatValue(key)}`
      )
    );
  }
  return mismatches;
}

function judgeCallbacks(
  expect: Expect,
  effects: RecordedEffects,
  allowUndeclared: boolean
): Mismatch[] {
  const expected = expect.callbacks ?? [];
  const actual = effects.callbacks;
  if (allowUndeclared) {
    if (isSubsequence(expected, actual)) {
      return [];
    }
    return [
      miss(
        "callbacks",
        "callbacks",
        formatValue(expected),
        formatValue(actual),
        "callbacks: expected callbacks did not appear in order"
      ),
    ];
  }
  const mismatches: Mismatch[] = [];
  const length = Math.max(expected.length, actual.length);
  for (let index = 0; index < length; index += 1) {
    const want = expected[index];
    const got = actual[index];
    if (deepEqual(want, got)) {
      continue;
    }
    mismatches.push(
      miss(
        "callbacks",
        `[${index}]`,
        want === undefined ? "(none)" : formatValue(want),
        got === undefined ? "(none)" : formatValue(got),
        `callbacks: [${index}] differed`
      )
    );
  }
  return mismatches;
}

function isSubsequence(
  expected: CallbackEffect[],
  actual: CallbackEffect[]
): boolean {
  let index = 0;
  for (const item of actual) {
    const want = expected[index];
    if (want !== undefined && deepEqual(want, item)) {
      index += 1;
    }
  }
  return index === expected.length;
}

function judgeOpenidm(
  expect: Expect,
  effects: RecordedEffects,
  strictness: Strictness
): Mismatch[] {
  const expected = expect.openidm ?? [];
  const mismatches: Mismatch[] = [];
  const declared = new Set<number>();
  for (const item of expected) {
    const matched = matchIndices(item, effects.openidm, openidmMatches);
    const times = item.times !== undefined ? item.times : 1;
    for (const index of matched) {
      declared.add(index);
    }
    if (matched.length !== times) {
      mismatches.push(
        miss(
          "openidm",
          formatOpenidmExpect(item),
          `${times}`,
          `${matched.length}`,
          `openidm: expected ${times} ${formatOpenidmExpect(item)}, actual ${matched.length}`
        )
      );
    }
  }
  for (let index = 0; index < effects.openidm.length; index += 1) {
    if (declared.has(index)) {
      continue;
    }
    const actual = effects.openidm[index];
    if (actual === undefined) {
      continue;
    }
    const write = WRITE_METHODS.has(actual.method);
    if (write && strictness.openidmWrites) {
      continue;
    }
    if (!write && strictness.openidmReads) {
      continue;
    }
    const kind = write ? "write" : "read";
    mismatches.push(
      miss(
        "openidm",
        formatOpenidm(actual),
        "(none)",
        formatOpenidm(actual),
        `openidm: undeclared ${kind} ${formatOpenidm(actual)}`
      )
    );
  }
  return mismatches;
}

function judgeHttp(
  expect: Expect,
  effects: RecordedEffects,
  allowUndeclared: boolean
): Mismatch[] {
  const expected = expect.http ?? [];
  const mismatches: Mismatch[] = [];
  const declared = new Set<number>();
  for (const item of expected) {
    const matched = matchIndices(item, effects.http, httpMatches);
    const times = item.times !== undefined ? item.times : 1;
    for (const index of matched) {
      declared.add(index);
    }
    if (matched.length !== times) {
      mismatches.push(
        miss(
          "http",
          formatHttpExpect(item),
          `${times}`,
          `${matched.length}`,
          `http: expected ${times} request matching ${formatHttpExpect(item)}, actual ${matched.length}`
        )
      );
    }
  }
  if (allowUndeclared) {
    return mismatches;
  }
  for (let index = 0; index < effects.http.length; index += 1) {
    if (declared.has(index)) {
      continue;
    }
    const actual = effects.http[index];
    if (actual === undefined) {
      continue;
    }
    mismatches.push(
      miss(
        "http",
        formatHttp(actual),
        "(none)",
        formatHttp(actual),
        `http: undeclared request ${formatHttp(actual)}`
      )
    );
  }
  return mismatches;
}

function judgeLogs(
  expect: Expect,
  effects: RecordedEffects,
  allowUndeclared: boolean
): Mismatch[] {
  const expected = expect.logs ?? [];
  const mismatches: Mismatch[] = [];
  const declared = new Set<number>();
  for (const item of expected) {
    const matched = matchIndices(item, effects.logs, logMatches);
    const times = item.times !== undefined ? item.times : 1;
    for (const index of matched) {
      declared.add(index);
    }
    if (matched.length !== times) {
      mismatches.push(
        miss(
          "logs",
          formatLogExpect(item),
          `${times}`,
          `${matched.length}`,
          `logs: expected ${times} line matching ${formatLogExpect(item)}, actual ${matched.length}`
        )
      );
    }
  }
  if (allowUndeclared) {
    return mismatches;
  }
  for (let index = 0; index < effects.logs.length; index += 1) {
    if (declared.has(index)) {
      continue;
    }
    const actual = effects.logs[index];
    if (actual === undefined) {
      continue;
    }
    mismatches.push(
      miss(
        "logs",
        formatLog(actual),
        "(none)",
        formatLog(actual),
        `logs: undeclared line ${formatLog(actual)}`
      )
    );
  }
  return mismatches;
}

function matchIndices<E, A>(
  expected: E,
  actuals: A[],
  matches: (expected: E, actual: A) => boolean
): number[] {
  const indices: number[] = [];
  for (let index = 0; index < actuals.length; index += 1) {
    const actual = actuals[index];
    if (actual !== undefined && matches(expected, actual)) {
      indices.push(index);
    }
  }
  return indices;
}

function openidmMatches(expected: OpenidmExpect, actual: OpenidmEffect): boolean {
  if (actual.method !== expected.method) {
    return false;
  }
  if (!matchesPattern(expected.resource, actual.resource)) {
    return false;
  }
  if (expected.body !== undefined && !deepEqual(expected.body, actual.body)) {
    return false;
  }
  if (expected.actionName !== undefined) {
    if (actual.actionName === undefined) {
      return false;
    }
    if (!matchesPattern(expected.actionName, actual.actionName)) {
      return false;
    }
  }
  return true;
}

function httpMatches(expected: HttpExpect, actual: HttpEffect): boolean {
  if (!matchesPattern(expected.url, actual.url)) {
    return false;
  }
  if (expected.method !== undefined && actual.method !== expected.method) {
    return false;
  }
  return true;
}

function logMatches(expected: LogExpect, actual: LogEffect): boolean {
  if (expected.level !== undefined && actual.level !== expected.level) {
    return false;
  }
  return matchesPattern(expected.message, actual.message);
}

function parseEffects(raw: unknown): RecordedEffects {
  if (!isPlainObject(raw)) {
    throw new Error("rhino-local: effects is not an object");
  }
  for (const key of EFFECTS_KEYS) {
    if (!(key in raw)) {
      throw new Error(
        `rhino-local: effects is missing ${key} — a runner that does not record a channel would silently assert nothing`
      );
    }
  }
  if (raw.outcome !== null && typeof raw.outcome !== "string") {
    throw new Error("rhino-local: effects.outcome must be a string or null");
  }
  return {
    outcome: raw.outcome,
    sharedState: parseStateBucket(raw.sharedState, "effects.sharedState"),
    transientState: parseStateBucket(
      raw.transientState,
      "effects.transientState"
    ),
    secureState: parseStateBucket(raw.secureState, "effects.secureState"),
    callbacks: parseArray(raw.callbacks, "effects.callbacks", parseCallbackEffect),
    openidm: parseArray(raw.openidm, "effects.openidm", parseOpenidmEffect),
    http: parseArray(raw.http, "effects.http", parseHttpEffect),
    logs: parseArray(raw.logs, "effects.logs", parseLogEffect),
  };
}

function parseStateBucket(raw: unknown, path: string): StateBucket {
  if (!isPlainObject(raw)) {
    throw new Error(`rhino-local: ${path} is not an object`);
  }
  if (!("initial" in raw) || !("final" in raw)) {
    throw new Error(`rhino-local: ${path} must have initial and final objects`);
  }
  return {
    initial: parseJsonObject(raw.initial, `${path}.initial`),
    final: parseJsonObject(raw.final, `${path}.final`),
  };
}

function parseCallbackEffect(raw: unknown, path: string): CallbackEffect {
  if (!isPlainObject(raw)) {
    throw new Error(`rhino-local: ${path} is not an object`);
  }
  if (typeof raw.type !== "string" || raw.type.trim() === "") {
    throw new Error(`rhino-local: ${path}.type must be a non-empty string`);
  }
  const callback: CallbackEffect = { type: raw.type };
  for (const [key, value] of Object.entries(raw)) {
    if (key === "type") {
      continue;
    }
    callback[key] = parseJsonValue(value, `${path}.${key}`);
  }
  return callback;
}

function parseOpenidmEffect(raw: unknown, path: string): OpenidmEffect {
  if (!isPlainObject(raw)) {
    throw new Error(`rhino-local: ${path} is not an object`);
  }
  if (typeof raw.method !== "string" || !OPENIDM_METHOD_SET.has(raw.method)) {
    throw new Error(
      `rhino-local: ${path}.method must be one of ${OPENIDM_METHODS.join(", ")}`
    );
  }
  if (typeof raw.resource !== "string") {
    throw new Error(`rhino-local: ${path}.resource must be a string`);
  }
  const effect: OpenidmEffect = {
    method: raw.method as OpenidmMethod,
    resource: raw.resource,
  };
  if (raw.body !== undefined) {
    effect.body = parseJsonValue(raw.body, `${path}.body`);
  }
  if (raw.actionName !== undefined) {
    if (typeof raw.actionName !== "string") {
      throw new Error(`rhino-local: ${path}.actionName must be a string`);
    }
    effect.actionName = raw.actionName;
  }
  return effect;
}

function parseHttpEffect(raw: unknown, path: string): HttpEffect {
  if (!isPlainObject(raw)) {
    throw new Error(`rhino-local: ${path} is not an object`);
  }
  if (typeof raw.url !== "string") {
    throw new Error(`rhino-local: ${path}.url must be a string`);
  }
  if (typeof raw.method !== "string" || raw.method.trim() === "") {
    throw new Error(`rhino-local: ${path}.method must be a non-empty string`);
  }
  const effect: HttpEffect = { url: raw.url, method: raw.method };
  if (raw.body !== undefined) {
    effect.body = parseJsonValue(raw.body, `${path}.body`);
  }
  return effect;
}

function parseLogEffect(raw: unknown, path: string): LogEffect {
  if (!isPlainObject(raw)) {
    throw new Error(`rhino-local: ${path} is not an object`);
  }
  if (typeof raw.level !== "string" || !LOG_LEVEL_SET.has(raw.level)) {
    throw new Error(
      `rhino-local: ${path}.level must be one of ${LOG_LEVELS.join(", ")}`
    );
  }
  if (typeof raw.message !== "string") {
    throw new Error(`rhino-local: ${path}.message must be a string`);
  }
  return { level: raw.level as LogLevel, message: raw.message };
}

function parseArray<T>(
  raw: unknown,
  path: string,
  item: (value: unknown, itemPath: string) => T
): T[] {
  if (!Array.isArray(raw)) {
    throw new Error(`rhino-local: ${path} is not an array`);
  }
  return raw.map((value, index) => item(value, `${path}[${index}]`));
}

function formatSummary(kase: Case, mismatches: Mismatch[]): string {
  const plural = mismatches.length === 1 ? "" : "es";
  const header = `${JSON.stringify(kase.name)} failed (${mismatches.length} mismatch${plural}):`;
  const body = mismatches
    .map(
      (item) =>
        `${item.message}\n  expected: ${item.expected}\n  actual: ${item.actual}`
    )
    .join("\n");
  return `${header}\n${body}`;
}

function formatOpenidm(op: OpenidmEffect): string {
  const action =
    op.actionName === undefined ? "" : ` action=${formatValue(op.actionName)}`;
  const body = op.body === undefined ? "" : ` body=${formatValue(op.body)}`;
  return `${op.method} ${op.resource}${action}${body}`;
}

function formatOpenidmExpect(item: OpenidmExpect): string {
  const resource =
    typeof item.resource === "string"
      ? item.resource
      : String(item.resource);
  const action =
    item.actionName === undefined
      ? ""
      : ` action=${typeof item.actionName === "string" ? formatValue(item.actionName) : String(item.actionName)}`;
  return `${item.method} ${resource}${action}`;
}

function formatHttp(op: HttpEffect): string {
  return `${op.method} ${op.url}`;
}

function formatHttpExpect(item: HttpExpect): string {
  const url = typeof item.url === "string" ? item.url : String(item.url);
  const method = item.method === undefined ? "" : `${item.method} `;
  return `${method}${url}`;
}

function formatLog(op: LogEffect): string {
  return `${op.level} ${formatValue(op.message)}`;
}

function formatLogExpect(item: LogExpect): string {
  const level = item.level === undefined ? "" : `${item.level} `;
  const message =
    typeof item.message === "string"
      ? formatValue(item.message)
      : String(item.message);
  return `${level}${message}`;
}

function miss(
  channel: Channel,
  path: string,
  expected: string,
  actual: string,
  message: string
): Mismatch {
  return { channel, path, expected, actual, message };
}
