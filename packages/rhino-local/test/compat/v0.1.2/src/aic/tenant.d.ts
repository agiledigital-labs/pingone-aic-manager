import { type HttpRequest, type HttpResponse } from "./http.ts";
import { type TenantProvider } from "./provider.ts";
export interface TenantSession {
    tenantName: string;
    baseUrl: string;
    token: string;
    /** Where `token` came from, and where a replacement comes from after a 401. */
    provider: TenantProvider;
}
export interface CliResult {
    status: number;
    stdout: string;
    stderr: string;
}
export interface AmResponse {
    status: number;
    bodyText: string;
    body: unknown;
    transactionId?: string;
}
export interface AmRequest {
    method: string;
    path: string;
    headers?: Array<[string, string]>;
    body?: string;
    /** When true, do not send the service-account bearer (authenticate). */
    anonymous?: boolean;
    timeoutMs?: number;
}
export interface AicIo {
    aic(args: string[]): Promise<CliResult>;
    http(req: HttpRequest): Promise<HttpResponse>;
}
export declare const AM_CONFIG_API_VERSION = "protocol=2.0,resource=1.0";
export declare function amConfigHeaders(): Array<[string, string]>;
export declare class AicLaneError extends Error {
    readonly status?: number;
    readonly transactionId?: string;
    constructor(message: string, options?: {
        status?: number;
        transactionId?: string;
        cause?: unknown;
    });
}
export declare function defaultAicIo(project: string): AicIo;
export interface ConnectOptions {
    /**
     * An `aic` context name. With the `aic` provider it selects the context;
     * with any other it must equal the provider's name, so a suite pinned to
     * one tenant cannot run against another.
     */
    tenant?: string;
    /** The `aic` project directory; the `aic` provider only. */
    project?: string;
    /** Default: {@link configuredTenantProvider}, else the `aic` CLI. */
    provider?: TenantProvider;
    /**
     * `false` skips the registered and environment providers. Callers pass it
     * when they were handed an `io`: an injected `io` is a test seam for both
     * `aic` and HTTP, and a configured service account would mint its token
     * over the real network behind it.
     */
    useConfigured?: boolean;
}
/**
 * The provider a connection uses: the one passed, else the one registered
 * with `setTenantProvider` or configured by `AIC_SCRIPT_TESTER_TENANT_URL`, else
 * this repo's `aic` agent.
 */
export declare function resolveTenantProvider(io: AicIo, options?: ConnectOptions): TenantProvider;
export declare function connectTenant(io: AicIo, options?: ConnectOptions): Promise<TenantSession>;
/**
 * This repo's provider: the tenant and bearer come from the `aic` agent
 * (`aic ctx list --json`, `aic whoami --token`), and logs from
 * `aic logs tx --wait`. The agent must be unlocked; every call passes
 * `--no-prompt` so a locked one fails instead of waiting for a password.
 */
export declare function aicCliProvider(io: AicIo, options?: {
    tenant?: string;
    project?: string;
}): TenantProvider;
/**
 * Ask the provider for a replacement bearer and adopt it.
 *
 * The history below is the `aic` provider's, and it is why a 401 must reach
 * the provider as `"rejected"`: a provider that caches has to know the token
 * it holds is dead, rather than handing it back.
 *
 * MEASURED 2026-09-14 over 20 minutes: `aic whoami --token` handed back a
 * token that worked *right now* and said nothing about how much life was left
 * in it. The first token this probe was given was rejected 401 five minutes
 * later, while a token fetched moments after it stayed good for the next ten —
 * the agent rotated on its own ~898s schedule, so what you got depended on
 * where in that cycle you asked.
 *
 * `aic` now guarantees `--token` at least 840 of those seconds, minting when
 * the cached token is below the floor (MEASURED 2026-09-15: with 823s left the
 * TTL jumped to 899 across the call, and with 864s left it did not move). That
 * removes the reason this refresh usually fired, and none of the reasons it
 * must stay: a file may run longer than the floor, the tenant may revoke, and
 * an older `aic` on someone else's PATH makes no such promise. So the 401 retry
 * remains the correctness mechanism and the floor is only what makes it rare.
 */
export declare function refreshSessionToken(session: TenantSession): Promise<void>;
export declare function realmJsonPath(realm: string): string;
export declare function amRequest(io: AicIo, session: TenantSession, req: AmRequest): Promise<AmResponse>;
export declare function redact(text: string, session: TenantSession): string;
