import type { RecordedEffects } from "../case/types.ts";
/**
 * Parse the JSON string `__rhinoLocalHarvest()` returns. The JVM runner can
 * only carry primitives as completion values (`JsValues` stringifies objects),
 * so harvest is a string on purpose.
 */
export declare function parseHarvest(raw: string): RecordedEffects;
