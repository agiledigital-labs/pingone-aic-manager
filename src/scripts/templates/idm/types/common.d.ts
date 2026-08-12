// Common IDM bindings shared by all IDM script families.
// Layered on top of rhino-1.7.14.d.ts.
//
// IDM binding availability is inferred from the prior template + docs; not yet
// runtime-probed (no local IDM sample corpus). See docs/api/12 IDM open items.

type LogFunction = (message: StringLike, ...args: any[]) => void;
interface Logger {
  trace: LogFunction;
  debug: LogFunction;
  info: LogFunction;
  warn: LogFunction;
  error: LogFunction;
}
declare const logger: Logger;

declare const console: {
  log(message?: any, ...args: any[]): void;
};

type Patch =
  | {
      operation: "add" | "replace";
      field: string;
      value: string;
    }
  | {
      operation: "remove";
      field: string;
    };

// The CREST call chain, shared by endpoint + schedule scripts. `context.http`
// is present iff an HTTP request sits at the ROOT of the context chain — so it
// is OPTIONAL even inside a custom endpoint (verified 2026-07-21):
//   • Direct REST call to the endpoint → present.
//   • Endpoint reached internally from ANOTHER endpoint (openidm.read/action)
//     whose origin was HTTP → still present; it's inherited and points at the
//     originating HTTP caller, not the inner hop.
//   • Endpoint reached from a non-HTTP origin — a scheduled job, recon/liveSync,
//     boot/startup, or any internal trigger → ABSENT. (Live-verified: a schedule
//     calling `openidm.action("endpoint/…")` saw `context.http === undefined`
//     in both the schedule script and the endpoint.)
// Always guard `context.http` before use — and `context.oauth2` likewise, which
// exists only when a validated bearer sat at the root. Remaining contexts
// (transactionId, session, current, parent, …) vary, hence the index signature.
interface IdmContext {
  http?: {
    method: string;
    path: string;
    headers: Record<string, string>;
    // Raw HTTP-layer query-param map: ALL query params, incl. `_`-prefixed
    // ones. (Contrast `request.additionalParameters`, the CREST-layer map of
    // NON-`_` params only.) Verified 2026-07-21.
    parameters: Record<string, string>;
  };
  security?: {
    authenticationId: string;
    authorization: { id: string; component: string; roles: string[] };
  };
  // Present only when `rsFilter` validated an OAuth2 bearer at the root of the
  // chain — so absent for a schedule, a recon/liveSync hook, and any internal
  // trigger. Guard before use. Keys verified 2026-08-06 (docs/api/11):
  // class, name, rawInfo, token, scopes, expiresAt, parent. Notably ABSENT:
  // `context.oauth2.scope` (singular) and `context.oauth2.accessToken`.
  oauth2?: {
    // The token's validated scopes, from Ping's AccessTokenInfo.getScopes().
    // A java.util.Set, so membership is `.contains("fr:idm:*")` — `.includes`
    // and `.indexOf` do not exist on it, and neither does `.length`.
    scopes: JavaSet<StringLike>;
    // AM's token-introspection record. Every member below was observed present
    // with the type given, verified 2026-08-07 against a service-account token
    // (docs/api/11). A user token carries the same keys; only the identity
    // values differ, and those variants are not yet verified.
    rawInfo: {
      active: boolean;
      auditTrackingId: string;
      authGrantId: string;
      /** The OAuth2 client the token was issued to. `service-account` for an SA. */
      client_id: string;
      /** Epoch SECONDS, not millis. */
      exp: number;
      /** Seconds remaining at introspection time. */
      expires_in: number;
      /** AM's internal issuer URL (e.g. `https://am.fr-platform:443/am/oauth2`),
       *  NOT the tenant base URL. Do not compare it against your tenant host. */
      iss: string;
      /** `/` for a root-realm token; a realm path otherwise. */
      realm: string;
      // The same scopes as `scopes` above, space-delimited. Prefer the set;
      // split this only if you need the raw string.
      scope: string;
      /** CREDENTIAL — never return or log. */
      sessionToken: string;
      sub: string;
      subname: string;
      token_type: string;
      /** For a service-account token these three all hold the SA's UUID. */
      user_id: string;
      username: string;
      [key: string]: any;
    };
    // CREDENTIAL — the bearer itself. Never return or log.
    token: string;
    expiresAt?: number;
    [key: string]: any;
  };
  [key: string]: any;
}
declare const context: IdmContext;

declare const identityServer: {
  getProperty(
    name: string,
    defaultValue?: string,
    substitute?: boolean
  ): string | null;
  getInstallLocation(): string;
  getProjectLocation(): string;
  getWorkingLocation(): string;
};

// The generated `types/managed/openidm-map.d.ts` fills this with
// collection-path → interface mappings for the tenant's managed objects.
// Single generic signatures + conditionals (instead of per-object overloads)
// keep field typos pinned to the offending element with spelling suggestions.
interface ManagedObjects {}

type ManagedName = keyof ManagedObjects & string;

// Field spec for `fields` args: schema property, `*`, or relationship paths
// such as `manager/displayName` and `_meta/lastChanged` (path + `*` syntax
// verified live — docs/api/10-managed-objects.md).
type ManagedField<T> =
  | (keyof T & string)
  | "*"
  | `${(keyof T & string) | "_meta"}/${string}`
  | "_meta";

interface QueryResult<T> {
  result: T[];
  resultCount: number;
  pagedResultsCookie: string | null;
  totalPagedResultsPolicy: string;
  totalPagedResults: number;
  remainingPagedResults: number;
}

// Collection path ("managed/<obj>") → object interface, else never.
type ManagedCollectionOf<R extends string> = R extends keyof ManagedObjects
  ? ManagedObjects[R]
  : never;

// Record path ("managed/<obj>/<id>") → object interface, else never.
type ManagedRecordOf<R extends string> =
  R extends `managed/${infer N}/${string}`
    ? `managed/${N}` extends keyof ManagedObjects
      ? ManagedObjects[`managed/${N}`]
      : never
    : never;

// fields: typed for known managed paths, rejected for unknown managed paths
// (nothing to check against — pull the schema again), free-form otherwise.
type FieldsArg<T, R extends string> = [T] extends [never]
  ? R extends `managed/${string}`
    ? never
    : string[]
  : ManagedField<T>[];

type ContentArg<T> = [T] extends [never] ? object : Partial<T>;

type RecordResult<T> = [T] extends [never] ? any : T;

// A record handed back by the store always carries `_id` and `_rev`: a managed
// object instance is "`_id` and `_rev` plus its declared properties"
// (docs/api/10-managed-objects.md). The generated managed interfaces still
// leave both optional, because the same interface types the onCreate hook's
// draft `object`, which has neither yet.
type StoredRecord<T> = T & { _id: string; _rev: string };

interface OpenIdm {
  read<R extends `${ManagedName}/${string}` | (string & {})>(
    path: R,
    params?: Record<string, string> | null
  ): [ManagedRecordOf<R>] extends [never]
    ? any
    : StoredRecord<ManagedRecordOf<R>> | null;
  // Restricting the returned fields drops the `_id`/`_rev` guarantee: we have
  // not verified that IDM still includes them, so this form keeps them optional.
  read<R extends `${ManagedName}/${string}` | (string & {})>(
    path: R,
    params: Record<string, string> | null | undefined,
    fields: FieldsArg<ManagedRecordOf<R>, R>
  ): [ManagedRecordOf<R>] extends [never] ? any : ManagedRecordOf<R> | null;
  query<R extends ManagedName | (string & {})>(
    path: R,
    params: { _queryFilter: string }
  ): [ManagedCollectionOf<R>] extends [never]
    ? any
    : QueryResult<ManagedCollectionOf<R>>;
  create<R extends ManagedName | (string & {})>(
    path: R,
    newResourceId: string | null,
    content: ContentArg<ManagedCollectionOf<R>> | null,
    params?: Record<string, string> | null
  ): RecordResult<ManagedCollectionOf<R>>;
  update<R extends `${ManagedName}/${string}` | (string & {})>(
    path: R,
    revision: string | null,
    content: ContentArg<ManagedRecordOf<R>> | null,
    params?: Record<string, string> | null
  ): RecordResult<ManagedRecordOf<R>>;
  patch<R extends `${ManagedName}/${string}` | (string & {})>(
    path: R,
    revision: string | null,
    patch: Patch[]
  ): RecordResult<ManagedRecordOf<R>>;
  delete<R extends `${ManagedName}/${string}` | (string & {})>(
    path: R,
    revision: string | null,
    params?: Record<string, string> | null
  ): RecordResult<ManagedRecordOf<R>>;
  action(
    path: string,
    actionName: string,
    content?: Record<string, any> | null,
    params?: Record<string, string> | null
  ): any;
}
declare const openidm: OpenIdm;
