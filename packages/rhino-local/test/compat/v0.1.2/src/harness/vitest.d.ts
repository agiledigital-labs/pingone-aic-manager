import type { z } from "zod";
import type { AicIo } from "../aic/tenant.ts";
import type { TenantProvider } from "../aic/provider.ts";
import { Lease, type LeaseOptions, type Suite } from "./lease.ts";
export interface UseLeaseAicOptions {
    id: string;
    realm?: string;
    tenant?: string;
    project?: string;
    /** Where the tenant and its bearer come from; see `resolveTenantProvider`. */
    provider?: TenantProvider;
    unsupported?: "fail" | "skip";
}
export interface UseLeaseOptions extends Partial<Omit<LeaseOptions, "runner" | "lane" | "realm">> {
    spawnTimeoutMs?: number;
    aic?: UseLeaseAicOptions;
    /** Injected fake seam for adapter tests; production uses the default AIC I/O. */
    aicIo?: AicIo;
    /** Injected filesystem seam for adapter tests. */
    aicStateDir?: string;
}
/** The env var that opts a run into the tenant lane. */
export declare const AIC_LANE_ENV = "AIC_SCRIPT_TESTER_AIC";
/**
 * Spread into `useLease` options to run both lanes when `AIC_SCRIPT_TESTER_AIC=1`.
 *
 * Off by default, deliberately. The tenant lane needs an unlocked agent and
 * network, which CI has neither of, and a checkout without them should still
 * be able to run the whole suite offline — so opting in is a per-invocation
 * decision, not a property of the file:
 *
 *     npm test                      # local lane only, as before
 *     AIC_SCRIPT_TESTER_AIC=1 npm test    # both lanes, against the sandbox
 *
 * `id` must be unique per file: it seeds the deterministic resource ids, and
 * one AIC lease per file is enforced.
 */
export declare function aicWhenEnabled(id: string, realm?: string): {
    aic?: UseLeaseAicOptions;
};
/**
 * Take a lease for one test file, and register its own lifecycle.
 *
 * The hooks are registered here rather than left to the author because
 * forgetting teardown is the one mistake that leaks tenant state, and it
 * leaks silently — the suite still passes. Owning the hooks makes the leak
 * impossible to cause by omission. The same applies to failure records: the
 * author must not have to remember to write the transaction id down.
 */
export declare function useLease<TSchema extends z.ZodType>(suite: Suite<TSchema>, options?: UseLeaseOptions): Lease<TSchema>;
export declare function claimAicLeaseForFile(file: string, id: string): void;
export declare function releaseAicLeaseForFile(file: string, id?: string): void;
