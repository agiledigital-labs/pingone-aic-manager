import { judge, validateCase } from "../case/index.ts";
import type { Case, RecordedEffects, Verdict } from "../case/types.ts";
import type { JobResponse } from "../protocol.ts";
import type { RhinoRunner } from "../runner.ts";
import { parseHarvest } from "./harvest.ts";
import { mockPreamble, withHarvest, type MockPreambleOptions } from "./preamble.ts";
import { decisionNodeClassAllowList } from "./allowlist.ts";
import { checkSeededManaged } from "../profile/seed.ts";
import type { EnvProfile } from "../profile/types.ts";

export interface RunCaseOptions {
  timeoutMs?: number;
  sourceName?: string;
  /** AM library script bodies keyed by `require()` id. See mockPreamble. */
  libraries?: Record<string, string>;
  /**
   * Pulled environment schema. Supplying one turns on strict fixture checking
   * and lets a declared-but-unseeded object type read as empty (AIC's `null`)
   * instead of a missing-fixture throw.
   */
  profile?: EnvProfile;
  /**
   * Install AM's Java class shutter. On by default: a scripted-decision case
   * should run behind the same allow-list the tenant enforces, or the harness
   * is more permissive than AIC and green means nothing. Pass `false` only to
   * demonstrate the difference.
   */
  classShutter?: boolean;
}

export interface CaseRun {
  effects: RecordedEffects;
  verdict: Verdict;
  response: JobResponse;
}

/**
 * Eval a case against the local Rhino runner: mocks as preamble, author script
 * as source, harvest appended so line numbers do not shift.
 */
export async function runCase(
  runner: RhinoRunner,
  input: Case,
  options: RunCaseOptions = {}
): Promise<CaseRun> {
  const kase = validateCase(input);
  if (options.profile !== undefined) {
    // Fail before the JVM sees the case: a fixture that names a property the
    // environment does not define is a test bug, and saying so here names the
    // case rather than surfacing as a puzzling mismatch downstream.
    checkSeededManaged(options.profile, kase.given.managed, kase.name);
  }
  const request: {
    source: string;
    sourceName: string;
    preamble: string;
    preambleName: string;
    timeoutMs?: number;
    classAllowList?: string[];
  } = {
    source: withHarvest(kase.script),
    sourceName: options.sourceName ?? kase.name,
    preamble: mockPreamble(kase.given, preambleOptions(options)),
    preambleName: "rhino-local-mocks.cjs",
  };
  if (options.timeoutMs !== undefined) {
    request.timeoutMs = options.timeoutMs;
  }
  // The legacy evaluator does not enforce the next-gen decision node's
  // allow-list: `legacy-es2015-globals` records `new java.util.HashMap()`
  // succeeding on a live legacy run, where next-gen hides it. Installing the
  // next-gen list there would invent a restriction the tenant does not apply.
  const shutterApplies =
    options.classShutter !== false && kase.given.engine !== "legacy";
  if (shutterApplies) {
    request.classAllowList = decisionNodeClassAllowList();
  }
  const response = await runner.eval(request);
  if (response.outcome !== "ok") {
    const detail = response.error?.message ?? response.outcome;
    throw new Error(
      `rhino-local: case ${JSON.stringify(kase.name)} ${response.outcome}: ${detail}`
    );
  }
  if (typeof response.value !== "string") {
    throw new Error(
      `rhino-local: harvest returned ${response.valueKind}, not a JSON string`
    );
  }
  const effects = parseHarvest(response.value);
  return { effects, verdict: judge(kase, effects), response };
}

function preambleOptions(options: RunCaseOptions): MockPreambleOptions {
  const out: MockPreambleOptions = {};
  if (options.libraries !== undefined) {
    out.libraries = options.libraries;
  }
  if (options.profile !== undefined) {
    out.profile = options.profile;
  }
  return out;
}
