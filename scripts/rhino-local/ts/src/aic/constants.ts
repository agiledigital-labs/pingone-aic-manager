/** Built-in Success node. Same id on every AM tenant (`docs/api/09-journeys.md`). */
export const SUCCESS_NODE_ID = "70e691a5-1e33-4ac3-a356-e7b6d60d92e0";

/** Built-in Failure node. Same id on every AM tenant (`docs/api/09-journeys.md`). */
export const FAILURE_NODE_ID = "e301438c-0bd0-429c-ab0c-66126501069a";

/**
 * HiddenValueCallback `id` the result script writes. The AIC runner reads
 * this callback back from `/authenticate` and strips it from `effects.callbacks`
 * so the harness dump is not mistaken for a subject-node callback.
 */
export const HARNESS_CALLBACK_ID = "__rhinoLocalEffects";

/** Prefix for throwaway tree and script names. Distinct from operator config. */
export const RESOURCE_PREFIX = "rl-aic";

export const SETUP_OUTCOME = "true";
