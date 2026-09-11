import type {
  CallbackEffect,
  Given,
  JsonObject,
  RecordedEffects,
  RecordingEvidence,
  StateChannel,
} from "../case/types.ts";
import { STATE_CHANNELS } from "../case/types.ts";
import { deepEqual } from "../case/equal.ts";
import { diffState } from "../case/state.ts";
import { isPlainObject, parseJsonObject } from "../case/util.ts";

const AIC_UNOBSERVED = ["openidm", "http", "logs"] as const;

export interface SubjectDump {
  outcome: string;
  before: JsonObject;
  final: JsonObject;
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
      throw new Error(`rhino-local: harness dump is not JSON: ${message}`);
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
  };
}

export function assembleEffects(args: {
  given: Given;
  dump?: SubjectDump;
  callbacks: CallbackEffect[];
}): RecordedEffects {
  const sharedInitial = args.given.sharedState ?? {};
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
      callbacks: args.callbacks,
      openidm: [],
      http: [],
      logs: [],
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
    args.dump.final
  );
  return {
    outcome: args.dump.outcome,
    sharedState: classified.sharedState,
    transientState: classified.transientState,
    secureState: classified.secureState,
    callbacks: args.callbacks,
    openidm: [],
    http: [],
    logs: [],
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
  final: JsonObject
): Pick<
  Required<RecordedEffects>,
  "sharedState" | "transientState" | "secureState" | "evidence"
> {
  const sharedInitial = given.sharedState ?? {};
  const transientInitial = given.transientState ?? {};
  const secureInitial = given.secureState ?? {};
  verifySeedsVisible(given, before);

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
  if (Object.prototype.hasOwnProperty.call(given.sharedState ?? {}, key)) {
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

function verifySeedsVisible(given: Given, before: JsonObject): void {
  const keys = new Set([
    ...Object.keys(given.sharedState ?? {}),
    ...Object.keys(given.secureState ?? {}),
    ...Object.keys(given.transientState ?? {}),
  ]);
  for (const key of keys) {
    const expected = visibleSeed(given, key);
    if (
      !Object.prototype.hasOwnProperty.call(before, key) ||
      !deepEqual(before[key], expected)
    ) {
      throw new Error(
        `rhino-local: AIC subject state did not contain the declared seed ${JSON.stringify(key)}`
      );
    }
  }
}

function visibleSeed(given: Given, key: string): unknown {
  // AM's unified lookup precedence is transient → secure → shared.
  if (Object.prototype.hasOwnProperty.call(given.transientState ?? {}, key)) {
    return given.transientState?.[key];
  }
  if (Object.prototype.hasOwnProperty.call(given.secureState ?? {}, key)) {
    return given.secureState?.[key];
  }
  return given.sharedState?.[key];
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
