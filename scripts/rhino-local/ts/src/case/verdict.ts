import { deepEqual, matchesPattern } from "./equal.ts";
import { isPortable } from "./portable.ts";
import {
  ALLOW_UNDECLARED_CHANNELS,
  DEFAULT_ALLOW_UNDECLARED,
  LOG_LEVELS,
  OPENIDM_METHODS,
  OPENIDM_WRITE_METHODS,
  STATE_CHANNELS,
} from "./types.ts";
import type {
  AllowUndeclared,
  AllowUndeclaredChannel,
  CallbackEffect,
  Case,
  Channel,
  EvidenceChannel,
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
  RecordingEvidence,
  StateBucket,
  StateChannel,
  StateDiff,
  StateMutation,
  UnbucketedStateMutation,
  Unverified,
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
const CHANNEL_SET: ReadonlySet<string> = new Set(EFFECTS_KEYS);
const STATE_CHANNEL_SET: ReadonlySet<string> = new Set(STATE_CHANNELS);

type Strictness = { [K in AllowUndeclaredChannel]: boolean };

/**
 * Pure function: (case, recorded effects) → pass/fail with a readable
 * explanation of each difference. Does not run a script.
 */
export function judge(input: unknown, effects: unknown): Verdict {
  const kase = validateCase(input);
  const recorded = parseEffects(effects);
  const strictness = resolveStrictness(kase.expect.allowUndeclared);
  const evidence = recorded.evidence ?? exactEvidence();
  const unobserved = new Set(evidence.unobservedChannels);
  const reconciled = reconcileUnbucketedState(
    kase.expect,
    evidence.unbucketedState,
    strictness
  );
  const unverified: Unverified[] = [
    ...reconciled.unverified,
    ...qualifyUnifiedState(kase.expect, strictness, evidence),
    ...qualifyUnobserved(kase.expect, strictness, evidence.unobservedChannels),
  ];
  const mismatches: Mismatch[] = [
    ...reconciled.mismatches,
    ...(unobserved.has("outcome") ? [] : judgeOutcome(kase, recorded)),
    ...(unobserved.has("sharedState") ? [] : judgeState(
      "sharedState",
      kase.expect.sharedState,
      recorded.sharedState,
      strictness.sharedState,
      reconciled.handled.sharedState
    )),
    ...(unobserved.has("transientState") ? [] : judgeState(
      "transientState",
      kase.expect.transientState,
      recorded.transientState,
      strictness.transientState,
      reconciled.handled.transientState
    )),
    ...(unobserved.has("secureState") ? [] : judgeState(
      "secureState",
      kase.expect.secureState,
      recorded.secureState,
      strictness.secureState,
      reconciled.handled.secureState
    )),
    ...(unobserved.has("callbacks")
      ? []
      : judgeCallbacks(kase.expect, recorded, strictness.callbacks)),
    ...(unobserved.has("openidm")
      ? []
      : judgeOpenidm(kase.expect, recorded, strictness)),
    ...(unobserved.has("http")
      ? []
      : judgeHttp(kase.expect, recorded, strictness.http)),
    ...(unobserved.has("logs")
      ? []
      : judgeLogs(kase.expect, recorded, strictness.logs)),
  ];
  return {
    pass: mismatches.length === 0,
    conclusive: unverified.length === 0,
    portable: isPortable(kase),
    mismatches,
    unverified,
    summary: mismatches.length === 0 ? "" : formatSummary(kase, mismatches),
  };
}

interface StateDeclaration {
  channel: StateChannel;
  key: string;
  operation: StateMutation["operation"];
  after?: JsonObject[string];
}

type HandledState = { [K in StateChannel]: Set<string> };

function reconcileUnbucketedState(
  expect: Expect,
  mutations: UnbucketedStateMutation[],
  strictness: Strictness
): {
  mismatches: Mismatch[];
  unverified: Unverified[];
  handled: HandledState;
} {
  const mismatches: Mismatch[] = [];
  const unverified: Unverified[] = [];
  const handled: HandledState = {
    sharedState: new Set(),
    transientState: new Set(),
    secureState: new Set(),
  };
  for (const mutation of mutations) {
    const declarations = stateDeclarations(expect, mutation.key);
    if (declarations.length === 0) {
      const closed = mutation.possibleBuckets.filter(
        (bucket) => !strictness[bucket]
      );
      if (closed.length > 0) {
        mismatches.push(
          miss(
            "nodeState",
            mutation.key,
            "(none)",
            formatMutation(mutation),
            `nodeState: undeclared ${formatMutation(mutation)}; bucket is unobservable (could be ${mutation.possibleBuckets.join(", ")})`
          )
        );
      }
      continue;
    }
    for (const declaration of declarations) {
      handled[declaration.channel].add(declaration.key);
    }
    if (declarations.length !== 1) {
      mismatches.push(
        miss(
          "nodeState",
          mutation.key,
          declarations.map(formatDeclaration).join(" or "),
          formatMutation(mutation),
          `nodeState: ${JSON.stringify(mutation.key)} has multiple bucket expectations, but AIC exposes one unified value`
        )
      );
      continue;
    }
    const declaration = declarations[0];
    if (declaration === undefined) {
      continue;
    }
    if (!mutation.possibleBuckets.includes(declaration.channel)) {
      mismatches.push(
        miss(
          declaration.channel,
          mutation.key,
          formatDeclaration(declaration),
          `${formatMutation(mutation)} in ${mutation.possibleBuckets.join(" or ")}`,
          `${declaration.channel}: ${JSON.stringify(mutation.key)} cannot be attributed to that bucket`
        )
      );
      continue;
    }
    if (!mutationMatchesDeclaration(mutation, declaration)) {
      mismatches.push(
        miss(
          declaration.channel,
          mutation.key,
          formatDeclaration(declaration),
          formatMutation(mutation),
          `${declaration.channel}: ${JSON.stringify(mutation.key)} differed in unified nodeState`
        )
      );
      continue;
    }
    unverified.push({
      channel: declaration.channel,
      path: mutation.key,
      message: `${declaration.channel}: ${JSON.stringify(mutation.key)} ${mutation.operation} value matched, but its bucket is unobservable (could be ${mutation.possibleBuckets.join(", ")})`,
    });
  }
  return { mismatches, unverified, handled };
}

function stateDeclarations(expect: Expect, key: string): StateDeclaration[] {
  const declarations: StateDeclaration[] = [];
  for (const channel of STATE_CHANNELS) {
    const state = expect[channel];
    if (Object.prototype.hasOwnProperty.call(state?.added ?? {}, key)) {
      const after = state?.added?.[key];
      if (after !== undefined) {
        declarations.push({ channel, key, operation: "added", after });
      }
    }
    if (Object.prototype.hasOwnProperty.call(state?.changed ?? {}, key)) {
      const after = state?.changed?.[key];
      if (after !== undefined) {
        declarations.push({ channel, key, operation: "changed", after });
      }
    }
    if ((state?.removed ?? []).includes(key)) {
      declarations.push({ channel, key, operation: "removed" });
    }
  }
  return declarations;
}

function mutationMatchesDeclaration(
  mutation: StateMutation,
  declaration: StateDeclaration
): boolean {
  if (mutation.operation !== declaration.operation) {
    return false;
  }
  if (mutation.operation === "removed") {
    return true;
  }
  return deepEqual(mutation.after, declaration.after);
}

function formatMutation(mutation: StateMutation): string {
  if (mutation.operation === "removed") {
    return `removed ${formatValue(mutation.key)}`;
  }
  if (mutation.operation === "added") {
    return `added ${formatValue(mutation.key)}=${formatValue(mutation.after)}`;
  }
  return `changed ${formatValue(mutation.key)} from ${formatValue(mutation.before)} to ${formatValue(mutation.after)}`;
}

function formatDeclaration(declaration: StateDeclaration): string {
  if (declaration.operation === "removed") {
    return `${declaration.channel} removed ${formatValue(declaration.key)}`;
  }
  return `${declaration.channel} ${declaration.operation} ${formatValue(declaration.key)}=${formatValue(declaration.after)}`;
}

function qualifyUnobserved(
  expect: Expect,
  strictness: Strictness,
  channels: Channel[]
): Unverified[] {
  const gaps: Unverified[] = [];
  for (const channel of channels) {
    if (!needsObservation(channel, expect, strictness)) {
      continue;
    }
    gaps.push({
      channel,
      path: channel,
      message: `${channel}: the runner cannot observe this channel, so its expectations or undeclared-effect policy were not verified`,
    });
  }
  return gaps;
}

function qualifyUnifiedState(
  expect: Expect,
  strictness: Strictness,
  evidence: RecordingEvidence
): Unverified[] {
  if (evidence.stateBuckets === "exact") {
    return [];
  }
  const needsBuckets =
    STATE_CHANNELS.some((channel) => stateDiffHasEntries(expect[channel])) ||
    STATE_CHANNELS.some((channel) => !strictness[channel]);
  if (!needsBuckets) {
    return [];
  }
  return [
    {
      channel: "nodeState",
      path: "buckets",
      message:
        "nodeState: the runner observes one unified view; per-bucket absence and hidden lower-precedence writes were not verified",
    },
  ];
}

function needsObservation(
  channel: Channel,
  expect: Expect,
  strictness: Strictness
): boolean {
  if (channel === "outcome") {
    return true;
  }
  if (channel === "sharedState" || channel === "transientState" || channel === "secureState") {
    return stateDiffHasEntries(expect[channel]) || !strictness[channel];
  }
  if (channel === "callbacks") {
    return (expect.callbacks?.length ?? 0) > 0 || !strictness.callbacks;
  }
  if (channel === "openidm") {
    return (
      (expect.openidm?.length ?? 0) > 0 ||
      !strictness.openidmWrites ||
      !strictness.openidmReads
    );
  }
  if (channel === "http") {
    return (expect.http?.length ?? 0) > 0 || !strictness.http;
  }
  return (expect.logs?.length ?? 0) > 0 || !strictness.logs;
}

function stateDiffHasEntries(diff: StateDiff | undefined): boolean {
  return (
    Object.keys(diff?.added ?? {}).length > 0 ||
    Object.keys(diff?.changed ?? {}).length > 0 ||
    (diff?.removed?.length ?? 0) > 0
  );
}

function exactEvidence(): RecordingEvidence {
  return {
    stateBuckets: "exact",
    ambientState: {},
    unbucketedState: [],
    unobservedChannels: [],
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
  // An outcome outside the declared vocabulary is a harness configuration
  // fault, not the script behaving differently from expectation, and it
  // supersedes the ordinary mismatch because the ordinary message ("expected
  // X, actual Y") invites you to go and look at Y's branch — when the real
  // problem is that Y is not wired into the journey at all. On a tenant this
  // is the difference between a readable failure and a bare 401.
  if (
    kase.outcomes !== undefined &&
    effects.outcome !== null &&
    !kase.outcomes.includes(effects.outcome)
  ) {
    const declared = kase.outcomes.map((name) => formatValue(name)).join(", ");
    return [
      miss(
        "outcome",
        "outcome",
        formatValue(kase.expect.outcome),
        actual,
        `outcome: script produced ${actual}, which case.outcomes does not declare [${declared}] — fix the typo or declare it; on a tenant an undeclared outcome answers 401 with no callback, identical to a compile error`
      ),
    ];
  }
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
  allowUndeclared: boolean,
  handled: ReadonlySet<string> = new Set()
): Mismatch[] {
  const actual = diffState(bucket.initial, bucket.final);
  const expectedAdded = expected?.added ?? {};
  const expectedChanged = expected?.changed ?? {};
  const expectedRemoved = expected?.removed ?? [];
  const expectedRemovedSet = new Set(expectedRemoved);
  const mismatches: Mismatch[] = [];

  for (const key of Object.keys(expectedAdded)) {
    if (handled.has(key)) {
      continue;
    }
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
    if (handled.has(key)) {
      continue;
    }
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
    if (handled.has(key)) {
      continue;
    }
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
  const recorded: RecordedEffects = {
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
  if (raw.evidence !== undefined) {
    recorded.evidence = parseRecordingEvidence(raw.evidence);
    for (const channel of recorded.evidence.unobservedChannels) {
      if (recordedChannelHasValues(recorded, channel)) {
        throw new Error(
          `rhino-local: effects.${channel} cannot contain values while evidence marks it unobserved`
        );
      }
    }
  }
  return recorded;
}

function parseRecordingEvidence(raw: unknown): RecordingEvidence {
  if (!isPlainObject(raw)) {
    throw new Error("rhino-local: effects.evidence is not an object");
  }
  for (const key of [
    "stateBuckets",
    "ambientState",
    "unbucketedState",
    "unobservedChannels",
  ]) {
    if (!(key in raw)) {
      throw new Error(`rhino-local: effects.evidence is missing ${key}`);
    }
  }
  const unobservedChannels = parseStringArray(
    raw.unobservedChannels,
    "effects.evidence.unobservedChannels",
    CHANNEL_SET
  ) as Channel[];
  if (raw.stateBuckets !== "exact" && raw.stateBuckets !== "unified") {
    throw new Error(
      'rhino-local: effects.evidence.stateBuckets must be "exact" or "unified"'
    );
  }
  return {
    stateBuckets: raw.stateBuckets,
    ambientState: parseJsonObject(
      raw.ambientState,
      "effects.evidence.ambientState"
    ),
    unbucketedState: parseArray(
      raw.unbucketedState,
      "effects.evidence.unbucketedState",
      parseUnbucketedMutation
    ),
    unobservedChannels,
  };
}

function parseUnbucketedMutation(
  raw: unknown,
  path: string
): UnbucketedStateMutation {
  if (!isPlainObject(raw)) {
    throw new Error(`rhino-local: ${path} is not an object`);
  }
  if (typeof raw.operation !== "string") {
    throw new Error(`rhino-local: ${path}.operation must be a string`);
  }
  if (typeof raw.key !== "string") {
    throw new Error(`rhino-local: ${path}.key must be a string`);
  }
  const possibleBuckets = parseStringArray(
    raw.possibleBuckets,
    `${path}.possibleBuckets`,
    STATE_CHANNEL_SET
  ) as StateChannel[];
  if (possibleBuckets.length === 0) {
    throw new Error(`rhino-local: ${path}.possibleBuckets must not be empty`);
  }
  if (raw.operation === "added") {
    return {
      operation: "added",
      key: raw.key,
      after: parseJsonValue(raw.after, `${path}.after`),
      possibleBuckets,
    };
  }
  if (raw.operation === "changed") {
    return {
      operation: "changed",
      key: raw.key,
      before: parseJsonValue(raw.before, `${path}.before`),
      after: parseJsonValue(raw.after, `${path}.after`),
      possibleBuckets,
    };
  }
  if (raw.operation === "removed") {
    return {
      operation: "removed",
      key: raw.key,
      before: parseJsonValue(raw.before, `${path}.before`),
      possibleBuckets,
    };
  }
  throw new Error(
    `rhino-local: ${path}.operation must be added, changed, or removed`
  );
}

function parseStringArray(
  raw: unknown,
  path: string,
  allowed: ReadonlySet<string>
): string[] {
  if (!Array.isArray(raw) || raw.some((value) => typeof value !== "string")) {
    throw new Error(`rhino-local: ${path} must be an array of strings`);
  }
  const values = raw as string[];
  for (const value of values) {
    if (!allowed.has(value)) {
      throw new Error(`rhino-local: ${path} contains unknown value ${formatValue(value)}`);
    }
  }
  return [...new Set(values)];
}

function recordedChannelHasValues(
  effects: RecordedEffects,
  channel: Channel
): boolean {
  if (channel === "outcome") {
    return effects.outcome !== null;
  }
  if (channel === "sharedState" || channel === "transientState" || channel === "secureState") {
    return !deepEqual(effects[channel].initial, effects[channel].final);
  }
  return effects[channel].length > 0;
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
  channel: EvidenceChannel,
  path: string,
  expected: string,
  actual: string,
  message: string
): Mismatch {
  return { channel, path, expected, actual, message };
}
