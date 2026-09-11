import { deepEqual } from "../case/equal.ts";
import { diffState, sameMutationValue } from "../case/state.ts";
import { STATE_CHANNELS } from "../case/types.ts";
import type {
  Case,
  Channel,
  EvidenceChannel,
  RecordedEffects,
  StateChannel,
  StateMutation,
  Verdict,
} from "../case/types.ts";
import { formatValue } from "../case/util.ts";
import { judge } from "../case/verdict.ts";

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
  aic: RecordedEffects
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
    disagreements,
    observationGaps
  );
  for (const channel of ["callbacks", "openidm", "http", "logs"] as const) {
    compareArray(
      channel,
      local[channel],
      aic[channel],
      localUnobserved,
      aicUnobserved,
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
  channel: "callbacks" | "openidm" | "http" | "logs",
  local: unknown[],
  aic: unknown[],
  localUnobserved: ReadonlySet<Channel>,
  aicUnobserved: ReadonlySet<Channel>,
  disagreements: EffectsDisagreement[],
  gaps: ObservationGap[]
): void {
  if (recordGapIfUnobserved(channel, localUnobserved, aicUnobserved, gaps)) {
    return;
  }
  const length = Math.max(local.length, aic.length);
  for (let index = 0; index < length; index += 1) {
    if (deepEqual(local[index], aic[index])) {
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

function compareState(
  local: RecordedEffects,
  aic: RecordedEffects,
  localUnobserved: ReadonlySet<Channel>,
  aicUnobserved: ReadonlySet<Channel>,
  disagreements: EffectsDisagreement[],
  gaps: ObservationGap[]
): void {
  const localMutations = collectState(local, localUnobserved, "local", gaps);
  const aicMutations = collectState(aic, aicUnobserved, "AIC", gaps);
  const remaining = aicMutations.slice();
  const pairedAicKeys = new Set<string>();
  for (const localMutation of localMutations) {
    const sameKey = remaining
      .map((candidate, index) => ({ candidate, index }))
      .filter(({ candidate }) => candidate.mutation.key === localMutation.mutation.key);
    const equal = sameKey.find(({ candidate }) =>
      sameMutationValue(localMutation.mutation, candidate.mutation)
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
    if (!sameMutationValue(localMutation.mutation, aicMutation.mutation)) {
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
