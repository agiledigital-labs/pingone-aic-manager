import { deepEqual } from "../case/equal.ts";
import { containsMatcher, equalExceptMatchers, matchesValue } from "../case/matcher.ts";
import { diffState, sameMutationValue } from "../case/state.ts";
import { STATE_CHANNELS } from "../case/types.ts";
import type {
  Case,
  Channel,
  EvidenceChannel,
  Expect,
  ExpectedValue,
  HttpEffect,
  OpenidmEffect,
  RecordedEffects,
  StateChannel,
  StateMutation,
  Verdict,
} from "../case/types.ts";
import { formatValue } from "../case/util.ts";
import { httpMatches, judge, openidmMatches } from "../case/verdict.ts";

export interface EffectsDisagreement {
  channel: EvidenceChannel;
  path: string;
  local: string;
  aic: string;
  message: string;
}

export interface ObservationGap {
  channel: EvidenceChannel;
  path: string;
  local: string;
  aic: string;
  message: string;
}

export interface EffectsComparison {
  disagreements: EffectsDisagreement[];
  observationGaps: ObservationGap[];
}

type LocatedMutation =
  | { kind: "bucketed"; bucket: StateChannel; mutation: StateMutation }
  | {
      kind: "unbucketed";
      possibleBuckets: StateChannel[];
      mutation: StateMutation;
    };

/** Compare observable effects without turning absent evidence into equality. */
export function diffRecordedEffects(
  local: RecordedEffects,
  aic: RecordedEffects,
  expected?: Expect,
  priorExpectations: readonly Expect[] = []
): EffectsComparison {
  const disagreements: EffectsDisagreement[] = [];
  const observationGaps: ObservationGap[] = [];
  const localUnobserved = new Set(local.evidence?.unobservedChannels ?? []);
  const aicUnobserved = new Set(aic.evidence?.unobservedChannels ?? []);
  const localUnified = local.evidence?.stateBuckets === "unified";
  const aicUnified = aic.evidence?.stateBuckets === "unified";

  if (localUnified || aicUnified) {
    observationGaps.push({
      channel: "nodeState",
      path: "buckets",
      local: localUnified ? "unified" : "exact",
      aic: aicUnified ? "unified" : "exact",
      message:
        "nodeState: per-bucket absence and hidden lower-precedence writes cannot be compared through a unified view",
    });
  }

  compareScalar(
    "outcome",
    local.outcome,
    aic.outcome,
    localUnobserved,
    aicUnobserved,
    disagreements,
    observationGaps
  );
  compareState(
    local,
    aic,
    localUnobserved,
    aicUnobserved,
    expected,
    priorExpectations,
    disagreements,
    observationGaps
  );
  compareScalar(
    "sessionProperties",
    local.sessionProperties,
    aic.sessionProperties,
    localUnobserved,
    aicUnobserved,
    disagreements,
    observationGaps
  );
  for (const channel of ["callbacks", "openidm", "http", "logs", "identityWrites"] as const) {
    compareArray(
      channel,
      local[channel],
      aic[channel],
      localUnobserved,
      aicUnobserved,
      expected,
      disagreements,
      observationGaps
    );
  }
  return { disagreements, observationGaps };
}

function compareScalar(
  channel: Channel,
  local: unknown,
  aic: unknown,
  localUnobserved: ReadonlySet<Channel>,
  aicUnobserved: ReadonlySet<Channel>,
  disagreements: EffectsDisagreement[],
  gaps: ObservationGap[]
): void {
  if (recordGapIfUnobserved(channel, localUnobserved, aicUnobserved, gaps)) {
    return;
  }
  if (!deepEqual(local, aic)) {
    disagreements.push(
      disagree(
        channel,
        channel,
        formatValue(local),
        formatValue(aic),
        `${channel}: local ${formatValue(local)}, AIC ${formatValue(aic)}`
      )
    );
  }
}

function compareArray(
  channel: "callbacks" | "openidm" | "http" | "logs" | "identityWrites",
  local: unknown[],
  aic: unknown[],
  localUnobserved: ReadonlySet<Channel>,
  aicUnobserved: ReadonlySet<Channel>,
  expected: Expect | undefined,
  disagreements: EffectsDisagreement[],
  gaps: ObservationGap[]
): void {
  if (recordGapIfUnobserved(channel, localUnobserved, aicUnobserved, gaps)) {
    return;
  }
  const length = Math.max(local.length, aic.length);
  for (let index = 0; index < length; index += 1) {
    const matcher = arrayMatcher(channel, index, local, aic, expected);
    if (equalExceptMatchers(matcher, local[index], aic[index])) {
      continue;
    }
    disagreements.push(
      disagree(
        channel,
        `[${index}]`,
        local[index] === undefined ? "(none)" : formatValue(local[index]),
        aic[index] === undefined ? "(none)" : formatValue(aic[index]),
        `${channel}: [${index}] differed`
      )
    );
  }
}

function arrayMatcher(
  channel: "callbacks" | "openidm" | "http" | "logs" | "identityWrites",
  index: number,
  localEffects: unknown[],
  aicEffects: unknown[],
  expected: Expect | undefined
): unknown {
  const local = localEffects[index];
  const aic = aicEffects[index];
  if (channel === "callbacks") {
    const declarations = expected?.callbacks ?? [];
    const localIndices = callbackIndices(declarations, localEffects);
    const aicIndices = callbackIndices(declarations, aicEffects);
    return declarations.find((item, declaredIndex) =>
      containsMatcher(item) &&
      localIndices[declaredIndex] === index &&
      aicIndices[declaredIndex] === index);
  }
  if (channel === "openidm") {
    const matches = expected?.openidm?.find((item) =>
      item.body !== undefined && containsMatcher(item.body) &&
      isOpenidmEffect(local, item.method) && isOpenidmEffect(aic, item.method) &&
      openidmMatches(item, local) && openidmMatches(item, aic));
    return matches?.body === undefined ? undefined : { body: matches.body };
  }
  if (channel === "http") {
    const matches = expected?.http?.find((item) =>
      item.body !== undefined && containsMatcher(item.body) &&
      isHttpEffect(local) && isHttpEffect(aic) &&
      httpMatches(item, local) && httpMatches(item, aic));
    return matches?.body === undefined ? undefined : { body: matches.body };
  }
  return undefined;
}

function callbackIndices(declarations: NonNullable<Expect["callbacks"]>, effects: unknown[]): number[] {
  const indices: number[] = [];
  let next = 0;
  for (const declaration of declarations) {
    while (next < effects.length && !matchesValue(declaration, effects[next])) {
      next += 1;
    }
    indices.push(next < effects.length ? next : -1);
    next += 1;
  }
  return indices;
}

function isOpenidmEffect(value: unknown, method: string): value is OpenidmEffect {
  return typeof value === "object" && value !== null && "method" in value &&
    value.method === method && "resource" in value && typeof value.resource === "string";
}

function isHttpEffect(value: unknown): value is HttpEffect {
  return typeof value === "object" && value !== null && "url" in value &&
    typeof value.url === "string";
}

function compareState(
  local: RecordedEffects,
  aic: RecordedEffects,
  localUnobserved: ReadonlySet<Channel>,
  aicUnobserved: ReadonlySet<Channel>,
  expected: Expect | undefined,
  priorExpectations: readonly Expect[],
  disagreements: EffectsDisagreement[],
  gaps: ObservationGap[]
): void {
  const localMutations = collectState(local, localUnobserved, "local", gaps);
  const aicMutations = collectState(aic, aicUnobserved, "AIC", gaps);
  // A callback response has no result-node state dump. Its empty state maps
  // are placeholders, not evidence that the local writes were absent.
  if (STATE_CHANNELS.every((bucket) => localUnobserved.has(bucket) || aicUnobserved.has(bucket))) {
    return;
  }
  const remaining = aicMutations.slice();
  const pairedAicKeys = new Set<string>();
  for (const localMutation of localMutations) {
    const sameKey = remaining
      .map((candidate, index) => ({ candidate, index }))
      .filter(({ candidate }) => candidate.mutation.key === localMutation.mutation.key);
    const equal = sameKey.find(({ candidate }) =>
      sameMutationWithMatchers(localMutation, candidate, expected, priorExpectations)
    );
    const index = equal?.index ?? sameKey[0]?.index ?? -1;
    if (index < 0) {
      if (
        aic.evidence?.stateBuckets === "unified" &&
        (unifiedBeforeHasKey(aic, localMutation.mutation.key) ||
          pairedAicKeys.has(localMutation.mutation.key))
      ) {
        gaps.push(hiddenMutationGap("AIC", localMutation));
      } else {
        disagreements.push(stateDisagreement(localMutation, undefined));
      }
      continue;
    }
    const aicMutation = remaining[index];
    remaining.splice(index, 1);
    if (aicMutation === undefined) {
      disagreements.push(stateDisagreement(localMutation, undefined));
      continue;
    }
    pairedAicKeys.add(aicMutation.mutation.key);
    if (!sameMutationWithMatchers(localMutation, aicMutation, expected, priorExpectations)) {
      disagreements.push(stateDisagreement(localMutation, aicMutation));
      continue;
    }
    if (localMutation.kind === "bucketed" && aicMutation.kind === "bucketed") {
      if (localMutation.bucket !== aicMutation.bucket) {
        disagreements.push(stateDisagreement(localMutation, aicMutation));
      }
      continue;
    }
    if (!locationsCompatible(localMutation, aicMutation)) {
      disagreements.push(stateDisagreement(localMutation, aicMutation));
      continue;
    }
    gaps.push({
      channel: "nodeState",
      path: localMutation.mutation.key,
      local: formatLocated(localMutation),
      aic: formatLocated(aicMutation),
      message: `nodeState: ${JSON.stringify(localMutation.mutation.key)} value and operation agree, but its bucket is unobservable`,
    });
  }
  for (const aicMutation of remaining) {
    if (
      local.evidence?.stateBuckets === "unified" &&
      unifiedBeforeHasKey(local, aicMutation.mutation.key)
    ) {
      gaps.push(hiddenMutationGap("local", aicMutation));
    } else {
      disagreements.push(stateDisagreement(undefined, aicMutation));
    }
  }
}

function sameMutationWithMatchers(
  localLocated: LocatedMutation,
  aicLocated: LocatedMutation,
  expected: Expect | undefined,
  prior: readonly Expect[]
): boolean {
  const local = localLocated.mutation;
  const aic = aicLocated.mutation;
  if (local.operation !== aic.operation || local.key !== aic.key) return false;
  const bucket = matchingBucket(localLocated, aicLocated);
  const after = bucket === undefined ? undefined :
    declarationValue(expected, bucket, local.key, local.operation);
  const before = bucket === undefined ? undefined : priorMatcher(prior, bucket, local.key);
  if (local.operation === "added" && aic.operation === "added") {
    return after === undefined ? sameMutationValue(local, aic) :
      equalWhenMatcherMatches(after, local.after, aic.after);
  }
  if (local.operation === "changed" && aic.operation === "changed") {
    return (before === undefined
      ? deepEqual(local.before, aic.before)
      : equalWhenMatcherMatches(before, local.before, aic.before)) &&
      (after === undefined
        ? deepEqual(local.after, aic.after)
        : equalWhenMatcherMatches(after, local.after, aic.after));
  }
  if (local.operation === "removed" && aic.operation === "removed") {
    return before === undefined ? sameMutationValue(local, aic) :
      equalWhenMatcherMatches(before, local.before, aic.before);
  }
  return false;
}

function matchingBucket(local: LocatedMutation, aic: LocatedMutation): StateChannel | undefined {
  if (local.kind === "bucketed" && aic.kind === "bucketed") {
    return local.bucket === aic.bucket ? local.bucket : undefined;
  }
  if (local.kind === "bucketed") {
    return aic.kind === "unbucketed" && aic.possibleBuckets.includes(local.bucket)
      ? local.bucket : undefined;
  }
  if (aic.kind === "bucketed") {
    return local.possibleBuckets.includes(aic.bucket) ? aic.bucket : undefined;
  }
  return undefined;
}

function equalWhenMatcherMatches(expected: unknown, local: unknown, aic: unknown): boolean {
  return matchesValue(expected as ExpectedValue, local) &&
    matchesValue(expected as ExpectedValue, aic)
    ? equalExceptMatchers(expected, local, aic)
    : deepEqual(local, aic);
}

function declarationValue(expect: Expect | undefined, bucket: StateChannel, key: string,
  operation: StateMutation["operation"]): unknown {
  if (expect === undefined || operation === "removed") return undefined;
  const value = expect[bucket]?.[operation]?.[key];
  return containsMatcher(value) ? value : undefined;
}

function priorMatcher(prior: readonly Expect[], bucket: StateChannel, key: string): unknown {
  for (const expect of prior.slice().reverse()) {
    const diff = expect[bucket];
    for (const operation of ["added", "changed"] as const) {
      if (Object.prototype.hasOwnProperty.call(diff?.[operation] ?? {}, key)) {
        const value = diff?.[operation]?.[key];
        return containsMatcher(value) ? value : undefined;
      }
    }
    if (diff?.removed?.includes(key)) return undefined;
  }
  return undefined;
}

function unifiedBeforeHasKey(effects: RecordedEffects, key: string): boolean {
  if (Object.prototype.hasOwnProperty.call(effects.evidence?.ambientState ?? {}, key)) {
    return true;
  }
  return STATE_CHANNELS.some((bucket) =>
    Object.prototype.hasOwnProperty.call(effects[bucket].initial, key)
  );
}

function hiddenMutationGap(
  unobservingLane: "local" | "AIC",
  observed: LocatedMutation
): ObservationGap {
  return {
    channel: "nodeState",
    path: observed.mutation.key,
    local: unobservingLane === "local" ? "hidden by unified view" : formatLocated(observed),
    aic: unobservingLane === "AIC" ? "hidden by unified view" : formatLocated(observed),
    message: `nodeState: ${JSON.stringify(observed.mutation.key)} may be a lower-precedence or same-value write hidden from the ${unobservingLane} unified view`,
  };
}

function collectState(
  effects: RecordedEffects,
  unobserved: ReadonlySet<Channel>,
  lane: string,
  gaps: ObservationGap[]
): LocatedMutation[] {
  const mutations: LocatedMutation[] = [];
  for (const bucket of STATE_CHANNELS) {
    if (unobserved.has(bucket)) {
      gaps.push({
        channel: bucket,
        path: bucket,
        local: lane === "local" ? "unobserved" : "not compared",
        aic: lane === "AIC" ? "unobserved" : "not compared",
        message: `${bucket}: ${lane} lane cannot observe this channel`,
      });
      continue;
    }
    for (const mutation of diffState(
      effects[bucket].initial,
      effects[bucket].final
    )) {
      mutations.push({ kind: "bucketed", bucket, mutation });
    }
  }
  for (const mutation of effects.evidence?.unbucketedState ?? []) {
    mutations.push({
      kind: "unbucketed",
      possibleBuckets: mutation.possibleBuckets,
      mutation,
    });
  }
  return mutations;
}

function locationsCompatible(a: LocatedMutation, b: LocatedMutation): boolean {
  if (a.kind === "bucketed" && b.kind === "unbucketed") {
    return b.possibleBuckets.includes(a.bucket);
  }
  if (a.kind === "unbucketed" && b.kind === "bucketed") {
    return a.possibleBuckets.includes(b.bucket);
  }
  if (a.kind === "unbucketed" && b.kind === "unbucketed") {
    return a.possibleBuckets.some((bucket) => b.possibleBuckets.includes(bucket));
  }
  return true;
}

function recordGapIfUnobserved(
  channel: Channel,
  local: ReadonlySet<Channel>,
  aic: ReadonlySet<Channel>,
  gaps: ObservationGap[]
): boolean {
  if (!local.has(channel) && !aic.has(channel)) {
    return false;
  }
  gaps.push({
    channel,
    path: channel,
    local: local.has(channel) ? "unobserved" : "observed",
    aic: aic.has(channel) ? "unobserved" : "observed",
    message: `${channel}: lanes cannot be compared because ${local.has(channel) ? "local" : "AIC"} did not observe this channel`,
  });
  return true;
}

function stateDisagreement(
  local: LocatedMutation | undefined,
  aic: LocatedMutation | undefined
): EffectsDisagreement {
  const key = local?.mutation.key ?? aic?.mutation.key ?? "state";
  return disagree(
    "nodeState",
    key,
    local === undefined ? "(none)" : formatLocated(local),
    aic === undefined ? "(none)" : formatLocated(aic),
    `nodeState: ${JSON.stringify(key)} differed`
  );
}

function formatLocated(value: LocatedMutation): string {
  const location =
    value.kind === "bucketed"
      ? value.bucket
      : `one of ${value.possibleBuckets.join(", ")}`;
  return `${formatMutation(value.mutation)} in ${location}`;
}

function formatMutation(mutation: StateMutation): string {
  if (mutation.operation === "removed") {
    return `removed (was ${formatValue(mutation.before)})`;
  }
  if (mutation.operation === "added") {
    return `added ${formatValue(mutation.after)}`;
  }
  return `changed ${formatValue(mutation.before)} → ${formatValue(mutation.after)}`;
}

function disagree(
  channel: EvidenceChannel,
  path: string,
  local: string,
  aic: string,
  message: string
): EffectsDisagreement {
  return { channel, path, local, aic, message };
}

export function judgeBoth(
  kase: Case,
  local: RecordedEffects,
  aic: RecordedEffects
): { local: Verdict; aic: Verdict } {
  return { local: judge(kase, local), aic: judge(kase, aic) };
}
