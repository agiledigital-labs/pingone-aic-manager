import { randomUUID } from "node:crypto";
import { isPortable } from "../case/portable.ts";
import { STATE_CHANNELS } from "../case/types.ts";
import type {
  Case,
  CallbackEffect,
  JsonObject,
  RecordedEffects,
  Verdict,
} from "../case/types.ts";
import type { RunResult } from "../harness/lease.ts";
import {
  diffRecordedEffects,
  type EffectsDisagreement,
  type ObservationGap,
} from "./diff.ts";
import { judge } from "../case/verdict.ts";
import { managedSeedMatches, type ManagedFixture } from "./managed.ts";
import { oneShotSubjectId, oneShotSubjectName } from "./emit-journey.ts";
import { discoverTenantCookieName, runAicChain, runAicLane, type AicReply } from "./run.ts";
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

/** The local lane's completed chain, ready to be checked by the AIC lane. */
export interface LocalChainResult {
  cases: readonly Case[];
  localEffects: readonly RecordedEffects[];
  replies: readonly (readonly AicReply[])[];
  /** Present only when the local result came through the lease fixture ledger. */
  managedFixtures?: readonly ManagedFixture[];
  /** Identity seeded by Lease.execute, with provenance retained for AIC. */
  harnessIdentity?: { scriptName: string; loggerScriptId?: string; oneShotRunId?: string; cookieName?: string };
}

export type AicChainRunner = (args: {
  cases: readonly Case[];
  source: string;
  replies: readonly (readonly AicReply[])[];
  managedFixtures?: readonly ManagedFixture[];
  runId?: string;
}) => Promise<readonly RecordedEffects[]>;

export interface ChainConformanceInput extends LocalChainResult {
  source: string;
  /** The file lease supplied the real uploaded name to the local lane. */
  harnessOwnsScriptName?: boolean;
  harnessOwnsLoggerScriptId?: boolean;
  harnessOwnsCookieName?: boolean;
  /** For a one-shot chain whose local cases used the generated subject name. */
  oneShotRunId?: string;
  /** Omit to skip; pass a runner, or `"tenant"` to use `runAicChain`. */
  aic?: AicChainRunner | "tenant";
}

export interface ChainPassReport extends ConformanceReport {
  /** 1-based position in the chain. */
  pass: number;
  final: boolean;
  /** False when the pass suspended before the wrapper could dump its effects. */
  aicObserved: boolean;
}

export interface ChainConformanceReport {
  name: string;
  portable: boolean;
  passes: ChainPassReport[];
  disagreements: EffectsDisagreement[];
  observationGaps: ObservationGap[];
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
  const aicSkip = aicUnsupportedReason(input.kase);
  const runId = input.aic === "tenant" && aicSkip === undefined
    ? randomUUID().replace(/-/g, "").slice(0, 12)
    : undefined;
  const cookieName = runId === undefined ? undefined : await discoverTenantCookieName();
  const localInput = runId === undefined ? input : {
    ...input,
    kase: {
      ...input.kase,
      given: {
        ...input.kase.given,
        scriptName: oneShotSubjectName(runId),
        loggerScriptId: oneShotSubjectId(runId),
        ...(cookieName === undefined ? {} : { cookieName }),
      },
    },
  };
  const local = await runLane(input.local, localInput, "no local runner provided (bindings lane is a separate slice)");
  const aicRunner: LaneRunner | undefined = input.aic === "tenant"
    ? ({ kase, source }) => runAicLane(kase, source, runId === undefined ? {} : { runId })
    : input.aic;
  const aic = aicSkip
    ? skipResult(aicSkip)
    : await runLane(aicRunner, input, "no AIC runner provided");

  const comparison =
    local.effects !== undefined && aic.effects !== undefined
      ? diffRecordedEffects(local.effects, aic.effects, input.kase.expect)
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

/**
 * Check an already-completed local chain against one AIC journey.
 *
 * The caller supplies every carried case and reply. This function deliberately
 * cannot compute either: making the AIC lane repeat the local carry rule would
 * hide a wrong mock when both lanes made the same guess.
 */
export async function conformChain(
  input: ChainConformanceInput
): Promise<ChainConformanceReport> {
  validateChainInput(input);
  const local = input.cases.map((kase, index) =>
    observedResult(kase, input.localEffects[index] as RecordedEffects)
  );
  const harnessOwnsManaged =
    input.managedFixtures !== undefined &&
    managedSeedMatches(
      (input.cases[0] as Case).given.managed,
      input.managedFixtures
    );
  const unsupported = input.cases.flatMap((kase) => {
    const reason = aicUnsupportedReason(kase, {
      harnessOwnsManaged,
      harnessOwnsScriptName: input.harnessOwnsScriptName === true || input.oneShotRunId !== undefined || input.harnessIdentity !== undefined,
      harnessOwnsLoggerScriptId: input.harnessOwnsLoggerScriptId === true || input.oneShotRunId !== undefined || input.harnessIdentity?.loggerScriptId !== undefined,
      harnessOwnsCookieName: input.harnessOwnsCookieName === true || input.harnessIdentity?.cookieName !== undefined,
    });
    return reason === undefined ? [] : [`${kase.name}: ${reason}`];
  });
  const aicRunner = input.aic === "tenant" ? tenantChainRunner : input.aic;
  let aic: LaneResult[];
  if (unsupported.length > 0) {
    aic = input.cases.map(() =>
      skipResult(`AIC lane skipped for the whole chain: ${unsupported.join("; ")}`)
    );
  } else if (aicRunner === undefined) {
    aic = input.cases.map(() => skipResult("no AIC chain runner provided"));
  } else {
    aic = await runChainLane(aicRunner, input);
  }

  const finalIndex = input.cases.length - 1;
  const passes = input.cases.map((kase, index) => {
    const isFinal = index === finalIndex;
    return chainPassReport(
      kase,
      index,
      isFinal,
      local[index] as LaneResult,
      aic[index] as LaneResult,
      input.cases.slice(0, index).map((prior) => prior.expect)
    );
  });
  return {
    name: (input.cases[finalIndex] as Case).name,
    portable: input.cases.every(isPortable),
    passes,
    disagreements: passes.flatMap((pass) => pass.disagreements),
    observationGaps: passes.flatMap((pass) => pass.observationGaps),
  };
}

/**
 * Convert `Lease.execute()` output without re-running or reconstructing it.
 * External one-shot callers may use this too; `useLease()` sends it to the
 * file's already-open `AicFileLease`, not to `runAicChain()`.
 */
export function chainFromRunResult(result: RunResult): LocalChainResult {
  return {
    cases: [...result.steps.map((step) => step.kase), result.kase],
    localEffects: [...result.steps.map((step) => step.effects), result.effects],
    replies: result.steps.map((step) => submittedToAicReplies(step.submitted)),
    managedFixtures: result.fixtures,
    ...(result.harnessIdentity === undefined ? {} : { harnessIdentity: result.harnessIdentity }),
  };
}

function submittedToAicReplies(
  submitted: readonly CallbackEffect[]
): AicReply[] {
  const replies: AicReply[] = [];
  for (const callback of submitted) {
    const value = callback.value;
    if (Object.prototype.hasOwnProperty.call(callback, "value") && value !== undefined) {
      replies.push({ type: callback.type, value });
    }
  }
  return replies;
}

function validateChainInput(input: ChainConformanceInput): void {
  if (input.cases.length === 0) {
    throw new Error("conformChain needs at least one case");
  }
  if (input.localEffects.length !== input.cases.length) {
    throw new Error(
      `conformChain: ${input.cases.length} cases need ${input.cases.length} local effects, got ${input.localEffects.length}`
    );
  }
  if (input.replies.length !== input.cases.length - 1) {
    throw new Error(
      `conformChain: ${input.cases.length} passes need ${input.cases.length - 1} reply sets, got ${input.replies.length}`
    );
  }
  if (input.harnessIdentity !== undefined) {
    const identity = input.harnessIdentity;
    for (const kase of input.cases) {
      if (kase.given.scriptName !== identity.scriptName || kase.given.loggerScriptId !== identity.loggerScriptId || kase.given.cookieName !== identity.cookieName) {
        throw new Error(`${kase.name}: local script identity differs from the lease-recorded identity`);
      }
    }
  }
  if (input.harnessIdentity?.oneShotRunId !== undefined && input.oneShotRunId !== undefined && input.oneShotRunId !== input.harnessIdentity.oneShotRunId) {
    throw new Error("conformChain: oneShotRunId differs from the lease-recorded run ID");
  }
  const oneShotRunId = input.oneShotRunId ?? input.harnessIdentity?.oneShotRunId;
  if (oneShotRunId !== undefined) {
    const actualName = oneShotSubjectName(oneShotRunId);
    const actualId = oneShotSubjectId(oneShotRunId);
    for (const kase of input.cases) {
      if (kase.given.scriptName !== actualName) {
        throw new Error(`${kase.name}: local scriptName must be ${JSON.stringify(actualName)} for one-shot conformance`);
      }
      if (kase.given.loggerScriptId !== actualId) {
        throw new Error(`${kase.name}: local loggerScriptId must be ${JSON.stringify(actualId)} for one-shot conformance`);
      }
    }
  }
}

async function runChainLane(
  runner: AicChainRunner,
  input: ChainConformanceInput
): Promise<LaneResult[]> {
  try {
    const runId = input.oneShotRunId ?? input.harnessIdentity?.oneShotRunId;
    const effects = await runner({
      cases: input.oneShotRunId === undefined && input.harnessIdentity === undefined ? input.cases : input.cases.map(withoutScriptIdentity),
      source: input.source,
      replies: input.replies,
      ...(runId === undefined ? {} : { runId }),
      ...(input.managedFixtures !== undefined
        ? { managedFixtures: input.managedFixtures }
        : {}),
    });
    if (effects.length !== input.cases.length) {
      throw new Error(
        `AIC chain runner returned ${effects.length} effects for ${input.cases.length} passes`
      );
    }
    return input.cases.map((kase, index) =>
      observedResult(kase, effects[index] as RecordedEffects)
    );
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    return input.cases.map(() => ({ error: message }));
  }
}

function withoutScriptIdentity(kase: Case): Case {
  const given = { ...kase.given };
  delete given.scriptName;
  delete given.loggerScriptId;
  delete given.cookieName;
  return { ...kase, given };
}

function observedResult(kase: Case, effects: RecordedEffects): LaneResult {
  return { effects, verdict: judge(kase, effects) };
}

function chainPassReport(
  kase: Case,
  index: number,
  final: boolean,
  local: LaneResult,
  aic: LaneResult,
  priorExpectations: readonly Case["expect"][]
): ChainPassReport {
  const comparison =
    final && local.effects !== undefined && aic.effects !== undefined
      ? diffRecordedEffects(local.effects, aic.effects, kase.expect, priorExpectations)
      : { disagreements: [], observationGaps: [] };
  if (!final && aic.effects !== undefined) {
    comparison.observationGaps.push(intermediatePassGap(kase));
  }
  return {
    pass: index + 1,
    final,
    aicObserved: final && aic.effects !== undefined &&
      !STATE_CHANNELS.every((channel) =>
        aic.effects?.evidence?.unobservedChannels.includes(channel)
      ),
    name: kase.name,
    portable: isPortable(kase),
    local,
    aic,
    disagreements: comparison.disagreements,
    observationGaps: comparison.observationGaps,
    ambientState: collectAmbient(local, aic),
  };
}

function intermediatePassGap(kase: Case): ObservationGap {
  return {
    channel: "nodeState",
    path: "pass",
    local: "observed",
    aic: "unobserved",
    message: `${kase.name}: intermediate AIC effects are unobserved because the wrapper cannot dump state without adding a script-visible callback`,
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

const tenantChainRunner: AicChainRunner = ({
  cases,
  source,
  replies,
  managedFixtures,
  runId,
}) =>
  runAicChain(cases, source, {
    replies,
    ...(runId === undefined ? {} : { runId }),
    ...(managedFixtures !== undefined ? { managedFixtures } : {}),
  });
