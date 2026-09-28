import { createPrivateKey, randomUUID, sign, type JsonWebKey } from "node:crypto";
import { readFileSync } from "node:fs";
import { headerValues, sendHttp, type HttpRequest, type HttpResponse } from "./http.ts";

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
  transaction(id: string, options?: { waitMs?: number }): Promise<unknown[]>;
}

/** Console-issued log API key pair (`docs/api/08-logs.md`). */
export interface LogKeys {
  id: string;
  secret: string;
}

export type HttpSend = (req: HttpRequest) => Promise<HttpResponse>;

export class TenantProviderError extends Error {
  constructor(message: string, options: { cause?: unknown } = {}) {
    super(message, options.cause !== undefined ? { cause: options.cause } : {});
    this.name = "TenantProviderError";
  }
}

// --- token callback ---------------------------------------------------------

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
export function tokenCallbackProvider(options: TokenCallbackOptions): TenantProvider {
  const description = describeUrl(options.baseUrl, options.name);
  const logs =
    options.logKeys === undefined
      ? undefined
      : logKeysReader(description.baseUrl, options.logKeys, options.http ?? sendHttp);
  return {
    describe: () => Promise.resolve(description),
    async getToken(request) {
      const token = (await options.getToken(request)).trim();
      if (token.length === 0) {
        throw new TenantProviderError("the token callback returned an empty token");
      }
      return token;
    },
    ...(logs === undefined ? {} : { logs }),
  };
}

// --- service account --------------------------------------------------------

/** `docs/api/00-auth.md`; a returned scope may be a re-ordered subset. */
export const DEFAULT_SERVICE_ACCOUNT_SCOPE = "fr:am:* fr:idm:* fr:idc:esv:*";

/** Assertions are rejected if `exp` is much past 180s (`docs/api/00-auth.md`). */
const ASSERTION_LIFETIME_S = 180;
/** Refresh this long before the tenant's `expires_in` runs out. */
const REFRESH_MARGIN_S = 60;

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
export function serviceAccountProvider(options: ServiceAccountOptions): TenantProvider {
  const description = describeUrl(options.baseUrl, options.name);
  const key = privateKeyFrom(options.jwk);
  const http = options.http ?? sendHttp;
  const now = options.now ?? (() => Date.now() / 1000);
  const tokenUrl = `${description.baseUrl}/am/oauth2/access_token`;
  let cached: { token: string; refreshAt: number } | undefined;

  async function mint(): Promise<string> {
    const issuedAt = Math.floor(now());
    const assertion = signAssertion(key, {
      iss: options.serviceAccountId,
      sub: options.serviceAccountId,
      aud: tokenUrl,
      exp: issuedAt + ASSERTION_LIFETIME_S,
      jti: randomUUID(),
    });
    const body = new URLSearchParams({
      client_id: "service-account",
      grant_type: "urn:ietf:params:oauth:grant-type:jwt-bearer",
      assertion,
      scope: options.scope ?? DEFAULT_SERVICE_ACCOUNT_SCOPE,
    }).toString();
    const response = await http({
      url: tokenUrl,
      method: "POST",
      headerLines: [
        ["Content-Type", "application/x-www-form-urlencoded"],
        ["Accept", "application/json"],
      ],
      body,
    });
    if (response.status !== 200) {
      // The body names the problem (`invalid_client`, a bad `aud`) and holds
      // no secret: the assertion went out in the request, not back.
      throw new TenantProviderError(
        `service-account token request to ${tokenUrl} returned ${response.status}: ${response.body.slice(0, 300)}`
      );
    }
    let parsed: unknown;
    try {
      parsed = JSON.parse(response.body);
    } catch (error) {
      throw new TenantProviderError("service-account token response is not JSON", {
        cause: error,
      });
    }
    const record = parsed as { access_token?: unknown; expires_in?: unknown };
    if (typeof record.access_token !== "string" || record.access_token.length === 0) {
      throw new TenantProviderError("service-account token response has no access_token");
    }
    const lifetime = typeof record.expires_in === "number" ? record.expires_in : 0;
    cached = {
      token: record.access_token,
      refreshAt: issuedAt + Math.max(0, lifetime - REFRESH_MARGIN_S),
    };
    return record.access_token;
  }

  const logs =
    options.logKeys === undefined
      ? undefined
      : logKeysReader(description.baseUrl, options.logKeys, http);
  return {
    describe: () => Promise.resolve(description),
    async getToken(request) {
      if (request.reason === "initial" && cached !== undefined && now() < cached.refreshAt) {
        return cached.token;
      }
      return mint();
    },
    ...(logs === undefined ? {} : { logs }),
  };
}

function privateKeyFrom(jwk: JsonWebKey | string): ReturnType<typeof createPrivateKey> {
  let parsed: JsonWebKey;
  try {
    parsed = typeof jwk === "string" ? (JSON.parse(jwk) as JsonWebKey) : jwk;
  } catch (error) {
    throw new TenantProviderError("the service-account JWK is not JSON", { cause: error });
  }
  if (parsed.kty !== "RSA" || typeof parsed.d !== "string") {
    throw new TenantProviderError(
      "the service-account JWK must be an RSA private key (kty RSA, with d)"
    );
  }
  try {
    return createPrivateKey({ key: parsed, format: "jwk" });
  } catch (error) {
    throw new TenantProviderError("the service-account JWK is not a usable RSA private key", {
      cause: error,
    });
  }
}

/** Compact JWS, RS256. Exported for tests that verify it with the public key. */
export function signAssertion(
  key: ReturnType<typeof createPrivateKey>,
  claims: Record<string, unknown>
): string {
  const header = base64url(JSON.stringify({ alg: "RS256", typ: "JWT" }));
  const payload = base64url(JSON.stringify(claims));
  const signature = sign("RSA-SHA256", Buffer.from(`${header}.${payload}`), key);
  return `${header}.${payload}.${signature.toString("base64url")}`;
}

function base64url(text: string): string {
  return Buffer.from(text, "utf8").toString("base64url");
}

// --- logs via API key -------------------------------------------------------

/** What `aic logs tx` reads by default (`src/logs/cli.rs`). */
export const DEFAULT_LOG_SOURCES = ["am-everything", "idm-everything"] as const;
const ACCESS_OUTCOME = "AM-ACCESS-OUTCOME";
const MAX_LOG_PAGES = 50;
/** `aic`'s spacing (`src/logs/api.rs`): the API allows 60 requests/min. */
const REQUEST_INTERVAL_MS = 1_050;
const MAX_429_RETRIES = 6;

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
export function logKeysReader(
  baseUrl: string,
  keys: LogKeys,
  http: HttpSend,
  options: LogReaderOptions = {}
): LogReader {
  const pause = options.sleep ?? sleep;
  const now = options.now ?? Date.now;
  // The rate limit is per environment, not per reader: every default reader
  // in the process shares one spacer, as `aic`'s process-global throttle does.
  // A test that injects a clock gets its own, so tests cannot delay each other.
  const spacer =
    options.sleep === undefined && options.now === undefined
      ? sharedSpacer
      : new RequestSpacer(pause, now);

  async function get(url: string): Promise<HttpResponse> {
    for (let retry = 0; ; retry += 1) {
      await spacer.slot();
      const response = await http({
        url,
        method: "GET",
        headerLines: [
          ["x-api-key", keys.id],
          ["x-api-secret", keys.secret],
          ["Accept", "application/json"],
        ],
      });
      if (response.status !== 429 || retry >= MAX_429_RETRIES) {
        return response;
      }
      const retryAfter = Number(headerValues(response.headers, "retry-after")[0]);
      const backoff = 1_000 * 2 ** Math.min(retry, 6);
      await pause(Math.max(Number.isFinite(retryAfter) ? retryAfter * 1_000 : 0, backoff));
    }
  }

  async function fetchAll(id: string): Promise<unknown[]> {
    const events: unknown[] = [];
    let cookie: string | undefined;
    for (let page = 0; page < MAX_LOG_PAGES; page += 1) {
      const params = new URLSearchParams({
        source: DEFAULT_LOG_SOURCES.join(","),
        transactionId: id,
      });
      if (cookie !== undefined) {
        params.set("_pagedResultsCookie", cookie);
      }
      const response = await get(`${baseUrl}/monitoring/logs?${params.toString()}`);
      if (response.status !== 200) {
        throw new TenantProviderError(
          `GET /monitoring/logs returned ${response.status}: ${response.body.slice(0, 300)}`
        );
      }
      let body: { result?: unknown; pagedResultsCookie?: unknown };
      try {
        body = JSON.parse(response.body) as typeof body;
      } catch (error) {
        throw new TenantProviderError("GET /monitoring/logs returned a body that is not JSON", {
          cause: error,
        });
      }
      if (!Array.isArray(body.result)) {
        throw new TenantProviderError("GET /monitoring/logs returned no result array");
      }
      events.push(...(body.result as unknown[]));
      cookie = typeof body.pagedResultsCookie === "string" ? body.pagedResultsCookie : undefined;
      if (cookie === undefined || cookie.length === 0) {
        return events;
      }
    }
    throw new TenantProviderError(
      `transaction ${id} spans more than ${MAX_LOG_PAGES} log pages; narrow it`
    );
  }

  return {
    async transaction(id, readOptions = {}) {
      const waitMs = readOptions.waitMs ?? 60_000;
      const started = now();
      let events = await fetchAll(id);
      // Back-to-back like `aic logs tx --wait`; the request spacing above is
      // what keeps the loop inside the rate limit.
      while (!hasAccessOutcome(events) && now() - started < waitMs) {
        events = await fetchAll(id);
      }
      return events;
    },
  };
}

/**
 * Hands out request slots at least {@link REQUEST_INTERVAL_MS} apart, in call
 * order. Concurrent callers queue on one chain, so two cannot read the same
 * "last request" time and wake together.
 */
class RequestSpacer {
  #chain: Promise<void> = Promise.resolve();
  #last: number | undefined;
  readonly #pause: (ms: number) => Promise<void>;
  readonly #now: () => number;
  constructor(pause: (ms: number) => Promise<void>, now: () => number) {
    this.#pause = pause;
    this.#now = now;
  }

  slot(): Promise<void> {
    const next = this.#chain.then(async () => {
      if (this.#last !== undefined) {
        const wait = this.#last + REQUEST_INTERVAL_MS - this.#now();
        if (wait > 0) {
          await this.#pause(wait);
        }
      }
      this.#last = this.#now();
    });
    this.#chain = next.catch(() => undefined);
    return next;
  }
}

const sharedSpacer = new RequestSpacer(sleep, Date.now);

function hasAccessOutcome(events: readonly unknown[]): boolean {
  return events.some(
    (event) =>
      (event as { payload?: { eventName?: unknown } } | null)?.payload?.eventName ===
      ACCESS_OUTCOME
  );
}

// --- environment ------------------------------------------------------------

/** The variables {@link providerFromEnv} reads. */
export const TENANT_ENV = {
  url: "RHINO_LOCAL_TENANT_URL",
  name: "RHINO_LOCAL_TENANT_NAME",
  serviceAccountId: "RHINO_LOCAL_SA_ID",
  jwk: "RHINO_LOCAL_SA_JWK",
  jwkFile: "RHINO_LOCAL_SA_JWK_FILE",
  scope: "RHINO_LOCAL_SA_SCOPE",
  logKeyId: "RHINO_LOCAL_LOG_KEY_ID",
  logKeySecret: "RHINO_LOCAL_LOG_KEY_SECRET",
} as const;

/**
 * A service-account provider from the environment, or `undefined` when
 * `RHINO_LOCAL_TENANT_URL` is unset. A partial configuration throws rather
 * than falling back, so a typo cannot quietly send the run to another tenant.
 */
export function providerFromEnv(env: NodeJS.ProcessEnv = process.env): TenantProvider | undefined {
  // Presence, not truthiness, decides whether a configuration was attempted:
  // `RHINO_LOCAL_SA_ID=` must not read as "unset" and fall through to `aic`.
  const empty = Object.values(TENANT_ENV).filter((name) => env[name] === "");
  if (empty.length > 0) {
    throw new TenantProviderError(
      `${empty.join(", ")} ${empty.length === 1 ? "is" : "are"} set but empty`
    );
  }
  const url = env[TENANT_ENV.url];
  if (url === undefined) {
    const stray = Object.values(TENANT_ENV).filter((name) => env[name] !== undefined);
    if (stray.length > 0) {
      // Falling back to `aic` here would run against whatever context it has
      // current — the very tenant these variables were set to avoid.
      throw new TenantProviderError(
        `${stray.join(", ")} ${stray.length === 1 ? "is" : "are"} set but ${TENANT_ENV.url} is not`
      );
    }
    return undefined;
  }
  const id = env[TENANT_ENV.serviceAccountId];
  const inline = env[TENANT_ENV.jwk];
  const file = env[TENANT_ENV.jwkFile];
  if (!id) {
    throw new TenantProviderError(
      `${TENANT_ENV.url} is set but ${TENANT_ENV.serviceAccountId} is not`
    );
  }
  if (!inline === !file) {
    throw new TenantProviderError(
      `${TENANT_ENV.url} is set: give exactly one of ${TENANT_ENV.jwk} or ${TENANT_ENV.jwkFile}`
    );
  }
  const jwk = inline ?? readFileSync(file as string, "utf8");
  const keyId = env[TENANT_ENV.logKeyId];
  const keySecret = env[TENANT_ENV.logKeySecret];
  if (!keyId !== !keySecret) {
    throw new TenantProviderError(
      `give both ${TENANT_ENV.logKeyId} and ${TENANT_ENV.logKeySecret}, or neither`
    );
  }
  const scope = env[TENANT_ENV.scope];
  const name = env[TENANT_ENV.name];
  return serviceAccountProvider({
    baseUrl: url,
    serviceAccountId: id,
    jwk,
    ...(scope ? { scope } : {}),
    ...(name ? { name } : {}),
    ...(keyId && keySecret ? { logKeys: { id: keyId, secret: keySecret } } : {}),
  });
}

// --- registration -----------------------------------------------------------

let registered: TenantProvider | undefined;

/**
 * Set the provider every tenant-lane connection in this process uses, e.g.
 * from a Vitest `setupFiles` module. Pass `undefined` to clear it.
 */
export function setTenantProvider(provider: TenantProvider | undefined): void {
  registered = provider;
}

/** The registered provider, else one from the environment, else `undefined`. */
export function configuredTenantProvider(
  env: NodeJS.ProcessEnv = process.env
): TenantProvider | undefined {
  return registered ?? providerFromEnv(env);
}

// --- helpers ----------------------------------------------------------------

function describeUrl(baseUrl: string, name: string | undefined): TenantDescription {
  let url: URL;
  try {
    url = new URL(baseUrl);
  } catch (error) {
    throw new TenantProviderError(`tenant base URL ${JSON.stringify(baseUrl)} is not a URL`, {
      cause: error,
    });
  }
  if (url.protocol !== "https:") {
    // The assertion, every bearer and the log keys travel to this origin.
    throw new TenantProviderError(
      `tenant base URL must be https (got ${url.protocol.replace(/:$/, "")})`
    );
  }
  if (url.pathname !== "/" && url.pathname !== "") {
    throw new TenantProviderError(
      `tenant base URL must have no path (got ${url.pathname}); the harness adds /am and /openidm`
    );
  }
  return { name: name ?? url.hostname, baseUrl: url.origin };
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => {
    setTimeout(resolve, ms);
  });
}
