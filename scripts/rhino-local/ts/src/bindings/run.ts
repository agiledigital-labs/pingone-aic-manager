import { judge, validateCase } from "../case/index.ts";
import type { Case, RecordedEffects, Verdict } from "../case/types.ts";
import type { JobResponse } from "../protocol.ts";
import type { RhinoRunner } from "../runner.ts";
import { parseHarvest } from "./harvest.ts";
import { mockPreamble, withHarvest } from "./preamble.ts";

export interface RunCaseOptions {
  timeoutMs?: number;
  sourceName?: string;
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
  const request: {
    source: string;
    sourceName: string;
    preamble: string;
    preambleName: string;
    timeoutMs?: number;
  } = {
    source: withHarvest(kase.script),
    sourceName: options.sourceName ?? kase.name,
    preamble: mockPreamble(kase.given),
    preambleName: "rhino-local-mocks.cjs",
  };
  if (options.timeoutMs !== undefined) {
    request.timeoutMs = options.timeoutMs;
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
