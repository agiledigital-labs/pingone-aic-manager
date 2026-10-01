import type { TenantProvider } from "../aic/provider.ts";
import { type AicIo } from "../aic/tenant.ts";
import type { EnvProfile } from "./types.ts";
export declare const MANAGED_CONFIG_ENDPOINT = "/openidm/config/managed";
export interface PullOptions {
    tenant?: string;
    project?: string;
    /** Where the tenant and its bearer come from; see `resolveTenantProvider`. */
    provider?: TenantProvider;
    io?: AicIo;
    now?: () => Date;
}
/**
 * Pull one environment's managed-object schema into a normalised profile.
 *
 * One call covers every managed object the tenant defines, which is why this
 * is worth more than per-object fixtures: the breadth comes free.
 */
export declare function pullProfile(options?: PullOptions): Promise<{
    profile: EnvProfile;
    path: string;
}>;
