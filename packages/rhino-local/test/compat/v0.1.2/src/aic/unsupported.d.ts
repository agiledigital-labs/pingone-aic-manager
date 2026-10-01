import type { Case } from "../case/types.ts";
/**
 * Why this case must not be executed on a tenant. `undefined` means the
 * wrapper journey can seed `given` faithfully enough that a green AIC run
 * is evidence, not coincidence.
 *
 * Environment-dependent inputs (`esv` / `secrets` / `managed` / `http`) are
 * skipped rather than silently running against whatever the tenant holds.
 * The one exception is managed state whose fixture-ledger provenance was
 * checked by the caller; later chain passes may then carry records the subject
 * itself created without turning the tenant lane back off.
 */
export declare function aicUnsupportedReason(kase: Case, options?: {
    harnessOwnsManaged?: boolean;
}): string | undefined;
