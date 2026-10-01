import type { TenantProvider } from "./provider.ts";
import type { LeaseLaneHooks } from "../harness/lease.ts";
import { type ChainConformanceReport, type LocalChainResult } from "./conform.ts";
import { type LeaseIdentity } from "./lease-identity.ts";
import { type AicIo } from "./tenant.ts";
export interface AicFileLeaseOptions {
    id: string;
    suiteName: string;
    source: string;
    outcomes: readonly string[];
    libraries?: Readonly<Record<string, string>>;
    realm?: string;
    tenant?: string;
    project?: string;
    /** Where the tenant and its bearer come from; see `resolveTenantProvider`. */
    provider?: TenantProvider;
    unsupported?: "fail" | "skip";
    io?: AicIo;
    /** Test seam; production state defaults to the machine temp directory. */
    stateDir?: string;
    /** Test seam for proving a live lock fails closed without a long wait. */
    lockTimeoutMs?: number;
}
export interface AicLeaseRunRequest extends LocalChainResult {
    source: string;
    hooks: LeaseLaneHooks;
}
export declare class AicFileLease {
    #private;
    constructor(options: AicFileLeaseOptions);
    get identity(): LeaseIdentity;
    open(): Promise<void>;
    run(request: AicLeaseRunRequest): Promise<ChainConformanceReport>;
    endTest(): Promise<void>;
    close(): Promise<void>;
}
