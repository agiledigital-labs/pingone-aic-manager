import type {
  CallbackEffect,
  Given,
  JsonObject,
  JsonValue,
  RecordedEffects,
} from "../case/types.ts";
import { isPlainObject, parseJsonObject, parseJsonValue } from "../case/util.ts";

export interface SubjectDump {
  outcome: string;
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
  if (!("final" in raw)) {
    throw new Error("rhino-local: harness dump is missing final");
  }
  return {
    outcome: raw.outcome,
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
    };
  }
  const buckets = classifyFinal(args.given, args.dump.final);
  return {
    outcome: args.dump.outcome,
    sharedState: buckets.sharedState,
    transientState: buckets.transientState,
    secureState: buckets.secureState,
    callbacks: args.callbacks,
    openidm: [],
    http: [],
    logs: [],
  };
}

/**
 * `nodeState.get` is unified (transient → secure → shared). New keys have no
 * bucket API, so they land in shared — `putShared` is the common path. A
 * subject that `putTransient`s a brand-new key will therefore disagree with
 * a local lane that tracked the method; that disagreement is a measured
 * observability gap, not a silent reclassification.
 */
export function classifyFinal(
  given: Given,
  final: JsonObject
): Pick<RecordedEffects, "sharedState" | "transientState" | "secureState"> {
  const sharedInitial = given.sharedState ?? {};
  const transientInitial = given.transientState ?? {};
  const secureInitial = given.secureState ?? {};
  const sharedFinal: JsonObject = {};
  const transientFinal: JsonObject = {};
  const secureFinal: JsonObject = {};

  for (const [key, value] of Object.entries(final)) {
    const json = parseJsonValue(value, `dump.final.${key}`);
    if (Object.prototype.hasOwnProperty.call(transientInitial, key)) {
      assign(transientFinal, key, json);
    } else if (Object.prototype.hasOwnProperty.call(secureInitial, key)) {
      assign(secureFinal, key, json);
    } else {
      assign(sharedFinal, key, json);
    }
  }

  return {
    sharedState: { initial: sharedInitial, final: sharedFinal },
    transientState: { initial: transientInitial, final: transientFinal },
    secureState: { initial: secureInitial, final: secureFinal },
  };
}

function assign(target: JsonObject, key: string, value: JsonValue): void {
  target[key] = value;
}
