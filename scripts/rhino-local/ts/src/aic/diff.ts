import { deepEqual } from "../case/equal.ts";
import type {
  Case,
  Channel,
  RecordedEffects,
  StateBucket,
  Verdict,
} from "../case/types.ts";
import { formatValue } from "../case/util.ts";
import { judge } from "../case/verdict.ts";

export interface EffectsDisagreement {
  channel: Channel;
  path: string;
  local: string;
  aic: string;
  message: string;
}

/**
 * Compare two `RecordedEffects` records. This is the conformance output:
 * where the lanes disagree, not whether either lane matched `expect`.
 * `judge()` still runs separately against the same records.
 */
export function diffRecordedEffects(
  local: RecordedEffects,
  aic: RecordedEffects
): EffectsDisagreement[] {
  return [
    ...diffScalar("outcome", "outcome", local.outcome, aic.outcome),
    ...diffBucket("sharedState", local.sharedState, aic.sharedState),
    ...diffBucket("transientState", local.transientState, aic.transientState),
    ...diffBucket("secureState", local.secureState, aic.secureState),
    ...diffArray("callbacks", local.callbacks, aic.callbacks),
    ...diffArray("openidm", local.openidm, aic.openidm),
    ...diffArray("http", local.http, aic.http),
    ...diffArray("logs", local.logs, aic.logs),
  ];
}

function diffScalar(
  channel: Channel,
  path: string,
  local: unknown,
  aic: unknown
): EffectsDisagreement[] {
  if (deepEqual(local, aic)) {
    return [];
  }
  return [
    disagree(
      channel,
      path,
      formatValue(local),
      formatValue(aic),
      `${channel}: local ${formatValue(local)}, AIC ${formatValue(aic)}`
    ),
  ];
}

function diffBucket(
  channel: "sharedState" | "transientState" | "secureState",
  local: StateBucket,
  aic: StateBucket
): EffectsDisagreement[] {
  return [
    ...diffScalar(channel, "initial", local.initial, aic.initial),
    ...diffScalar(channel, "final", local.final, aic.final),
  ];
}

function diffArray(channel: Channel, local: unknown[], aic: unknown[]): EffectsDisagreement[] {
  if (deepEqual(local, aic)) {
    return [];
  }
  const mismatches: EffectsDisagreement[] = [];
  const length = Math.max(local.length, aic.length);
  for (let index = 0; index < length; index += 1) {
    if (deepEqual(local[index], aic[index])) {
      continue;
    }
    mismatches.push(
      disagree(
        channel,
        `[${index}]`,
        local[index] === undefined ? "(none)" : formatValue(local[index]),
        aic[index] === undefined ? "(none)" : formatValue(aic[index]),
        `${channel}: [${index}] differed`
      )
    );
  }
  return mismatches;
}

function disagree(
  channel: Channel,
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
