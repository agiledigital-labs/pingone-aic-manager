import { isPortable } from "../case/portable.ts";
import type { Case, JsonObject, RecordedEffects, Verdict } from "../case/types.ts";
import {
  diffRecordedEffects,
  type EffectsDisagreement,
  type ObservationGap,
} from "./diff.ts";
import { judge } from "../case/verdict.ts";
import { runAicLane } from "./run.ts";
import { aicUnsupportedReason } from "./unsupported.ts";

export type LaneRunner = (args: {
  kase: Case;
  source: string;
}) => Promise<RecordedEffects>;

export interface ConformanceInput {
  kase: Case;
  source: string;
  local?: LaneRunner;
  /** Omit to skip; pass a runner, or `"tenant"` to use `runAicLane`. */
  aic?: LaneRunner | "tenant";
}

export interface LaneResult {
  effects?: RecordedEffects;
  verdict?: Verdict;
  skipped?: string;
  error?: string;
}

export interface ConformanceReport {
  name: string;
  portable: boolean;
  local: LaneResult;
  aic: LaneResult;
  disagreements: EffectsDisagreement[];
  observationGaps: ObservationGap[];
  ambientState: Array<{ lane: "local" | "aic"; values: JsonObject }>;
}

/**
 * Run one case on both lanes and diff the two `RecordedEffects`.
 * Both lanes are judged by `judge()` from verdict.ts — a second pass/fail
 * implementation here would make the comparison prove nothing.
 *
 * Either lane may be omitted. A missing local runner is the expected state
 * until the bindings slice lands; a missing AIC runner is used by tests.
 */
export async function conform(input: ConformanceInput): Promise<ConformanceReport> {
  const local = await runLane(input.local, input, "no local runner provided (bindings lane is a separate slice)");
  const aicSkip = aicUnsupportedReason(input.kase);
  const aicRunner = input.aic === "tenant" ? tenantRunner : input.aic;
  const aic = aicSkip
    ? skipResult(aicSkip)
    : await runLane(aicRunner, input, "no AIC runner provided");

  const comparison =
    local.effects !== undefined && aic.effects !== undefined
      ? diffRecordedEffects(local.effects, aic.effects)
      : { disagreements: [], observationGaps: [] };

  return {
    name: input.kase.name,
    portable: isPortable(input.kase),
    local,
    aic,
    disagreements: comparison.disagreements,
    observationGaps: comparison.observationGaps,
    ambientState: collectAmbient(local, aic),
  };
}

function collectAmbient(
  local: LaneResult,
  aic: LaneResult
): ConformanceReport["ambientState"] {
  const ambient: ConformanceReport["ambientState"] = [];
  for (const [lane, result] of [
    ["local", local],
    ["aic", aic],
  ] as const) {
    const values = result.effects?.evidence?.ambientState;
    if (values !== undefined && Object.keys(values).length > 0) {
      ambient.push({ lane, values });
    }
  }
  return ambient;
}

async function runLane(
  runner: LaneRunner | undefined,
  input: ConformanceInput,
  missing: string
): Promise<LaneResult> {
  if (runner === undefined) {
    return skipResult(missing);
  }
  try {
    const effects = await runner({ kase: input.kase, source: input.source });
    return { effects, verdict: judge(input.kase, effects) };
  } catch (error) {
    return { error: error instanceof Error ? error.message : String(error) };
  }
}

function skipResult(reason: string): LaneResult {
  return { skipped: reason };
}

const tenantRunner: LaneRunner = ({ kase, source }) => runAicLane(kase, source);
