import { normaliseEffects } from "../case/verdict.ts";
import type { CompleteRecordedEffects } from "../case/types.ts";

/**
 * Parse the JSON string `__rhinoLocalHarvest()` returns. The JVM runner can
 * only carry primitives as completion values (`JsValues` stringifies objects),
 * so harvest is a string on purpose.
 *
 * The same validation as `normaliseEffects`, so serialised effects round-trip
 * with their evidence: a channel marked unobserved stays unobserved, and a
 * 0.1.2-shaped document without the later channels is read as unobserved
 * there rather than rejected.
 */
export function parseHarvest(raw: string): CompleteRecordedEffects {
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw) as unknown;
  } catch (error) {
    throw new Error(
      `rhino-local: harvest is not JSON: ${error instanceof Error ? error.message : String(error)}`,
      { cause: error }
    );
  }
  return normaliseEffects(parsed);
}
