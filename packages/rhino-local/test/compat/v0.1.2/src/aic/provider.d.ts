import { createPrivateKey, type JsonWebKey } from "node:crypto";
import { type HttpRequest, type HttpResponse } from "./http.ts";
/**
 * Where the tenant lane gets its tenant and its bearer.
 *
 * This is the one seam between the harness and whatever holds the
 * credentials. In this repo that is the `aic` agent ({@link aicCliProvider});
 * outside it, a service account ({@link serviceAccountProvider}) or the
 * caller's own token source ({@link tokenCallbackProvider}). Everything past
 * this seam — journey upload, `/authenticate`, IDM seeding, teardown — is the
 * same code whichever provider supplied the bearer.
 */
export interface TenantProvider {
    /** Where the tenant is. Called once per connection, before any token. */
    describe(): Promise<TenantDescription>;
    /**
     * A bearer for AM and IDM calls. Called with `reason: "initial"` when a
     * connection opens, and again with `reason: "rejected"` after a
     * non-anonymous call came back 401 — so a provider that caches must not
     * hand back the token that was just refused.
     */
    getToken(request: TokenRequest): Promise<string>;
    /** Present when this provider can read the tenant's logs (`show-log`). */
    readonly logs?: LogReader;
}
export interface TenantDescription {
    /** Recorded in failure dumps and traces; `show-log` matches on it. */
    name: string;
    /** `https://<tenant>` — no trailing slash, no path. */
    baseUrl: string;
}
export interface TokenRequest {
    reason: "initial" | "rejected";
    /** The bearer the tenant refused, when `reason` is `"rejected"`. */
    rejected?: string;
}
export interface LogReader {
    /**
     * Every event for one transaction id, polling until AM's trailing
     * `AM-ACCESS-OUTCOME` event arrives or `waitMs` elapses — logs lag the
     * request by tens of seconds. Whatever has arrived is returned on timeout.
     */
    transaction(id: string, options?: {
        waitMs?: number;
    }): Promise<unknown[]>;
}
/** Console-issued log API key pair (`docs/api/08-logs.md`). */
export interface LogKeys {
    id: string;
    secret: string;
}
export type HttpSend = (req: HttpRequest) => Promise<HttpResponse>;
export declare class TenantProviderError extends Error {
    constructor(message: string, options?: {
        cause?: unknown;
    });
}
export interface TokenCallbackOptions {
    baseUrl: string;
    /** Default: the base URL's hostname. */
    name?: string;
    getToken: (request: TokenRequest) => Promise<string> | string;
    logKeys?: LogKeys;
    http?: HttpSend;
}
/**
 * The caller supplies the bearer. The harness calls `getToken` when a
 * connection opens and again after a 401, and does no caching of its own.
 */
export declare function tokenCallbackProvider(options: TokenCallbackOptions): TenantProvider;
/** `docs/api/00-auth.md`; a returned scope may be a re-ordered subset. */
export declare const DEFAULT_SERVICE_ACCOUNT_SCOPE = "fr:am:* fr:idm:* fr:idc:esv:*";
export interface ServiceAccountOptions {
    baseUrl: string;
    /** The service account's UUID — `iss` and `sub` of the assertion. */
    serviceAccountId: string;
    /** The RSA private key the console issued, as a JWK object or JSON text. */
    jwk: JsonWebKey | string;
    scope?: string;
    name?: string;
    logKeys?: LogKeys;
    http?: HttpSend;
    /** Seconds since the epoch; injected by tests. */
    now?: () => number;
}
/**
 * Mint bearers with the JWT-bearer grant, as the `aic` agent does: an RS256
 * assertion (`iss` = `sub` = the account id, `aud` = the token endpoint, a
 * short `exp`, a fresh `jti`) posted to the root realm's
 * `/am/oauth2/access_token` as `client_id=service-account`.
 *
 * The token is cached until {@link REFRESH_MARGIN_S} before its
 * `expires_in`; a `"rejected"` request always mints a new one.
 */
export declare function serviceAccountProvider(options: ServiceAccountOptions): TenantProvider;
/** Compact JWS, RS256. Exported for tests that verify it with the public key. */
export declare function signAssertion(key: ReturnType<typeof createPrivateKey>, claims: Record<string, unknown>): string;
/** What `aic logs tx` reads by default (`src/logs/cli.rs`). */
export declare const DEFAULT_LOG_SOURCES: readonly ["am-everything", "idm-everything"];
export interface LogReaderOptions {
    /** Injected by tests; default `setTimeout`. */
    sleep?: (ms: number) => Promise<void>;
    /** Milliseconds since the epoch; injected by tests. */
    now?: () => number;
}
/**
 * `GET /monitoring/logs?source=…&transactionId=…` with the `x-api-key` /
 * `x-api-secret` pair, following `pagedResultsCookie` — the query
 * `aic logs tx` makes, with its request spacing and its `Retry-After`-honouring
 * 429 backoff. The log API rejects bearers (`docs/api/08-logs.md`), which is
 * why this needs its own keys.
 *
 * `transactionId` is a **prefix** match (`docs/api/08-logs.md`), as it is for
 * `aic logs tx`: asking for a chain's stem returns every pass under it, which
 * is what `show-log`'s "whole chain" choice relies on.
 */
export declare function logKeysReader(baseUrl: string, keys: LogKeys, http: HttpSend, options?: LogReaderOptions): LogReader;
/** The variables {@link providerFromEnv} reads. */
export declare const TENANT_ENV: {
    readonly url: "AIC_SCRIPT_TESTER_TENANT_URL";
    readonly name: "AIC_SCRIPT_TESTER_TENANT_NAME";
    readonly serviceAccountId: "AIC_SCRIPT_TESTER_SA_ID";
    readonly jwk: "AIC_SCRIPT_TESTER_SA_JWK";
    readonly jwkFile: "AIC_SCRIPT_TESTER_SA_JWK_FILE";
    readonly scope: "AIC_SCRIPT_TESTER_SA_SCOPE";
    readonly logKeyId: "AIC_SCRIPT_TESTER_LOG_KEY_ID";
    readonly logKeySecret: "AIC_SCRIPT_TESTER_LOG_KEY_SECRET";
};
/**
 * A service-account provider from the environment, or `undefined` when
 * `AIC_SCRIPT_TESTER_TENANT_URL` is unset. A partial configuration throws rather
 * than falling back, so a typo cannot quietly send the run to another tenant.
 */
export declare function providerFromEnv(env?: NodeJS.ProcessEnv): TenantProvider | undefined;
/**
 * Set the provider every tenant-lane connection in this process uses, e.g.
 * from a Vitest `setupFiles` module. Pass `undefined` to clear it.
 */
export declare function setTenantProvider(provider: TenantProvider | undefined): void;
/** The registered provider, else one from the environment, else `undefined`. */
export declare function configuredTenantProvider(env?: NodeJS.ProcessEnv): TenantProvider | undefined;
/**
 * Re-check what a provider says about its tenant. Every built-in provider
 * validates its URL when it is made, but a caller's own `TenantProvider` is
 * only a promise, and `connectTenant` sends a bearer to whatever this returns.
 */
export declare function checkTenantDescription(description: TenantDescription): TenantDescription;
/**
 * The OAuth `error` / `error_description` of a refused token request, with the
 * tenant's origin and hostname replaced. The description is what names the
 * problem (a bad `aud` quotes the URL it expected), but the message ends up in
 * test output and CI logs, where the tenant's hostname does not belong.
 */
export declare function tokenErrorDetail(body: string, baseUrl: string): string;
/** `text` with the tenant's origin and hostname replaced by `<tenant>`. */
export declare function scrubTenant(text: string, baseUrl: string): string;
