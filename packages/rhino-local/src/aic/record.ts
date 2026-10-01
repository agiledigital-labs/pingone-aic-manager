import type {
  CallbackEffect,
  Expect,
  Given,
  JsonObject,
  RecordedEffects,
  RecordingEvidence,
  StateChannel,
} from "../case/types.ts";
import { STATE_CHANNELS } from "../case/types.ts";
import { deepEqual } from "../case/equal.ts";
import { containsMatcher, seedMatches } from "../case/matcher.ts";
import { diffState } from "../case/state.ts";
import { formatValue, isPlainObject, parseJsonObject } from "../case/util.ts";

// `identityWrites`: the lane sees only what the script itself reads back. A
// lane-side read of the managed record would have to translate AM attribute
// names to IDM fields, and what store() leaves on that record is unmeasured
// (docs/api/14-am-identity-attributes.md → "Identity writes").
const AIC_UNOBSERVED = ["openidm", "http", "logs", "sessionProperties", "identityWrites"] as const;

function sharedSeed(given: Given): JsonObject {
  return given.registeredObjectAttributes === undefined
    ? { ...(given.sharedState ?? {}) }
    : { ...(given.sharedState ?? {}), objectAttributes: given.registeredObjectAttributes };
}

export interface SubjectDump {
  outcome: string;
  before: JsonObject;
  final: JsonObject;
  leaseDigest?: string;
  invocationNonce?: string;
  subjectDigest?: string;
}

/**
 * Parse the result-script payload. The script records `outcome` and the
 * unified `nodeState` map; this module classifies keys into shared/transient/
 * secure using `given`, so the dump does not reimplement the verdict rule.
 */
export function parseSubjectDump(raw: unknown): SubjectDump {
  if (typeof raw === "string") {
    let parsed: unknown;
    try {
      parsed = JSON.parse(raw);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      throw new Error(`rhino-local: harness dump is not JSON: ${message}`, { cause: error });
    }
    return parseSubjectDump(parsed);
  }
  if (!isPlainObject(raw)) {
    throw new Error("rhino-local: harness dump is not an object");
  }
  if (typeof raw.outcome !== "string") {
    throw new Error("rhino-local: harness dump.outcome must be a string");
  }
  if (!("before" in raw) || !("final" in raw)) {
    throw new Error("rhino-local: harness dump must have before and final");
  }
  return {
    outcome: raw.outcome,
    before: parseJsonObject(raw.before, "dump.before"),
    final: parseJsonObject(raw.final, "dump.final"),
    ...(typeof raw.leaseDigest === "string" ? { leaseDigest: raw.leaseDigest } : {}),
    ...(typeof raw.invocationNonce === "string"
      ? { invocationNonce: raw.invocationNonce }
      : {}),
    ...(typeof raw.subjectDigest === "string"
      ? { subjectDigest: raw.subjectDigest }
      : {}),
  };
}

export function assembleEffects(args: {
  given: Given;
  dump?: SubjectDump;
  callbacks: CallbackEffect[];
  priorExpectations?: readonly Expect[];
}): RecordedEffects {
  const sharedInitial = sharedSeed(args.given);
  const transientInitial = args.given.transientState ?? {};
  const secureInitial = args.given.secureState ?? {};
  if (args.dump === undefined) {
    // Subject suspended with callbacks: the result node never ran, so we
    // did not observe nodeState. Claiming every given key was removed would
    // be a false mutation.
    return {
      outcome: null,
      sharedState: { initial: sharedInitial, final: { ...sharedInitial } },
      transientState: { initial: transientInitial, final: { ...transientInitial } },
      secureState: { initial: secureInitial, final: { ...secureInitial } },
      sessionProperties: { initial: args.given.existingSession ?? {}, final: { ...(args.given.existingSession ?? {}) } },
      callbacks: args.callbacks,
      openidm: [],
      http: [],
      logs: [],
      identityWrites: [],
      evidence: {
        stateBuckets: "unified",
        ambientState: {},
        unbucketedState: [],
        unobservedChannels: [
          "sharedState",
          "transientState",
          "secureState",
          ...AIC_UNOBSERVED,
        ],
      },
    };
  }
  const classified = classifyFinal(
    args.given,
    args.dump.before,
    args.dump.final,
    args.priorExpectations
  );
  return {
    outcome: args.dump.outcome,
    sharedState: classified.sharedState,
    transientState: classified.transientState,
    secureState: classified.secureState,
    sessionProperties: { initial: args.given.existingSession ?? {}, final: { ...(args.given.existingSession ?? {}) } },
    callbacks: args.callbacks,
    openidm: [],
    http: [],
    logs: [],
    identityWrites: [],
    evidence: classified.evidence,
  };
}

/**
 * Classify only what the AIC wrapper actually established. The snapshots use
 * unified `nodeState.keys/get`, so a changed or added value is observable but
 * its bucket is not. Declared seeds remain exact only while unchanged; a seed
 * missing afterwards is an exact removal only when it existed in one bucket.
 * State present before the subject but absent from `given` is ambient.
 */
export function classifyFinal(
  given: Given,
  before: JsonObject,
  final: JsonObject,
  priorExpectations: readonly Expect[] = []
): Pick<
  Required<RecordedEffects>,
  "sharedState" | "transientState" | "secureState" | "evidence"
> {
  const sharedInitial = sharedSeed(given);
  const transientInitial = { ...(given.transientState ?? {}) };
  const secureInitial = { ...(given.secureState ?? {}) };
  verifySeedsVisible(given, before, priorExpectations);

  // Once a prior matcher permits a different tenant value, carry that observed
  // value into the AIC effects rather than reporting the local random seed.
  for (const key of Object.keys(before)) {
    if (priorMatcherForSeed(given, key, priorExpectations) === undefined) continue;
    const value = before[key];
    if (value === undefined) continue;
    if (Object.prototype.hasOwnProperty.call(transientInitial, key)) {
      transientInitial[key] = value;
    } else if (Object.prototype.hasOwnProperty.call(secureInitial, key)) {
      secureInitial[key] = value;
    } else if (Object.prototype.hasOwnProperty.call(sharedInitial, key)) {
      sharedInitial[key] = value;
    }
  }

  const sharedFinal: JsonObject = { ...sharedInitial };
  const transientFinal: JsonObject = { ...transientInitial };
  const secureFinal: JsonObject = { ...secureInitial };
  const ambientState: JsonObject = {};
  for (const [key, value] of Object.entries(before)) {
    if (seedBuckets(given, key).length === 0) {
      ambientState[key] = value;
    }
  }

  const unbucketedState: RecordingEvidence["unbucketedState"] = [];
  for (const mutation of diffState(before, final)) {
    const buckets = seedBuckets(given, mutation.key);
    if (mutation.operation === "removed" && buckets.length === 1) {
      const bucket = buckets[0];
      if (bucket !== undefined) {
        delete finalFor(bucket, sharedFinal, transientFinal, secureFinal)[mutation.key];
      }
      continue;
    }
    unbucketedState.push({
      ...mutation,
      possibleBuckets:
        buckets.length === 0 && mutation.operation === "added"
          ? ["sharedState", "transientState"]
          : [...STATE_CHANNELS],
    });
  }

  return {
    sharedState: { initial: sharedInitial, final: sharedFinal },
    transientState: { initial: transientInitial, final: transientFinal },
    secureState: { initial: secureInitial, final: secureFinal },
    evidence: {
      stateBuckets: "unified",
      ambientState,
      unbucketedState,
      unobservedChannels: [...AIC_UNOBSERVED],
    },
  };
}

function seedBuckets(given: Given, key: string): StateChannel[] {
  const buckets: StateChannel[] = [];
  if (Object.prototype.hasOwnProperty.call(sharedSeed(given), key)) {
    buckets.push("sharedState");
  }
  if (Object.prototype.hasOwnProperty.call(given.transientState ?? {}, key)) {
    buckets.push("transientState");
  }
  if (Object.prototype.hasOwnProperty.call(given.secureState ?? {}, key)) {
    buckets.push("secureState");
  }
  return buckets;
}

function verifySeedsVisible(given: Given, before: JsonObject, priorExpectations: readonly Expect[]): void {
  const keys = new Set([
    ...Object.keys(sharedSeed(given)),
    ...Object.keys(given.secureState ?? {}),
    ...Object.keys(given.transientState ?? {}),
  ]);
  for (const key of keys) {
    const expected = visibleSeed(given, key);
    const matcher = priorMatcherForSeed(given, key, priorExpectations);
    if (
      !Object.prototype.hasOwnProperty.call(before, key) ||
      (matcher === undefined
        ? !deepEqual(before[key], expected)
        : !seedMatches(matcher, expected, before[key]))
    ) {
      throw new Error(
        `rhino-local: AIC subject state did not contain the declared seed ${JSON.stringify(key)}${matcher === undefined ? "" : ` matching ${formatValue(matcher)}`}`
      );
    }
  }
}

function priorMatcherForSeed(given: Given, key: string, prior: readonly Expect[]): unknown {
  const bucket = Object.prototype.hasOwnProperty.call(given.transientState ?? {}, key)
    ? "transientState"
    : Object.prototype.hasOwnProperty.call(given.secureState ?? {}, key)
      ? "secureState" : "sharedState";
  for (const expect of prior.slice().reverse()) {
    const diff = expect[bucket];
    if (Object.prototype.hasOwnProperty.call(diff?.added ?? {}, key)) {
      const value = diff?.added?.[key];
      return containsMatcher(value) ? value : undefined;
    }
    if (Object.prototype.hasOwnProperty.call(diff?.changed ?? {}, key)) {
      const value = diff?.changed?.[key];
      return containsMatcher(value) ? value : undefined;
    }
    if (diff?.removed?.includes(key)) return undefined;
  }
  return undefined;
}

function visibleSeed(given: Given, key: string): unknown {
  // AM's unified lookup precedence is transient → secure → shared.
  if (Object.prototype.hasOwnProperty.call(given.transientState ?? {}, key)) {
    return given.transientState?.[key];
  }
  if (Object.prototype.hasOwnProperty.call(given.secureState ?? {}, key)) {
    return given.secureState?.[key];
  }
  return sharedSeed(given)[key];
}

function finalFor(
  bucket: StateChannel,
  shared: JsonObject,
  transient: JsonObject,
  secure: JsonObject
): JsonObject {
  if (bucket === "sharedState") {
    return shared;
  }
  if (bucket === "transientState") {
    return transient;
  }
  return secure;
}
