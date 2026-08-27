// Common IDM bindings shared by all IDM script families.
// Layered on top of rhino-1.7.14.d.ts.
//
// IDM binding availability is inferred from the prior template + docs; not yet
// runtime-probed (no local IDM sample corpus). See docs/api/12 IDM open items.

// slf4j-style. `LogFunction` comes from rhino-1.7.14.d.ts and counts the `{}`
// in the message, so a call that would silently leave a bare `{}` in the log —
// or silently drop an argument — fails to compile. See that file for what is
// measured on AM and what is carried over here.
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

// What `openidm.query` hands back. NOT the shape an endpoint may RETURN: IDM
// requires `remainingPagedResults` on a query handler's return value (see
// `IdmQueryResult` in endpoint.d.ts) but never sent it on a script-side query
// result in any observed response (verified 2026-08-17), hence optional here.
interface QueryResult<T> {
  result: T[];
  resultCount: number;
  pagedResultsCookie: string | null;
  totalPagedResultsPolicy: string;
  totalPagedResults: number;
  remainingPagedResults?: number;
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
// `readonly` so a `const` type parameter can capture the literal members; that
// inference is what makes `Projected` below possible.
type FieldsArg<T, R extends string> = [T] extends [never]
  ? R extends `managed/${string}`
    ? never
    : readonly string[]
  : readonly ManagedField<T>[];

type ContentArg<T> = [T] extends [never] ? object : Partial<T>;

type RecordResult<T> = [T] extends [never] ? any : T;

// A record handed back by the store always carries `_id` and `_rev`: a managed
// object instance is "`_id` and `_rev` plus its declared properties"
// (docs/api/10-managed-objects.md). The generated managed interfaces still
// leave both optional, because the same interface types the onCreate hook's
// draft `object`, which has neither yet.
type StoredRecord<T> = T & { _id: string; _rev: string };

// What a `parent/child` selector hands back for `parent`: the relationship
// REFERENCE envelope plus the requested members of the target (verified
// 2026-08-17 with `_meta/lastChanged`, which returned `_ref`,
// `_refResourceCollection`, `_refResourceId`, `_refResourceRev`, the target's own
// `_id`/`_rev`, and `lastChanged`). The target's schema is not recorded anywhere
// these types can reach — a generated interface says `manager?: RelationshipRef`,
// not which object it points at — so requested members stay index-only.
interface RelationshipExpansion {
  _id?: string;
  _rev?: string;
  _ref?: string;
  _refResourceCollection?: string;
  _refResourceId?: string;
  _refResourceRev?: string;
  _refProperties?: { _id?: string; _rev?: string };
  [member: string]: unknown;
}

// The `parent` of every requested `parent/child` path.
type PathParentOf<F extends string> = F extends `${infer P}/${string}`
  ? P
  : never;

// `_meta` is a relationship in its own right (to `managed/<realm>usermeta`), so
// asking for it adds an expansion rather than a declared property.
type MetaMemberOf<F extends string> = [
  Extract<F, "_meta" | `_meta/${string}`>,
] extends [never]
  ? unknown
  : { _meta: RelationshipExpansion | null };

// A relationship's CARDINALITY decides the projected shape (verified
// 2026-08-17): single-valued comes back as the expansion or `null` when unset,
// multi-valued as an array — `[]` when unset, never `null`. Typing a
// single-valued one as always present cost a live 500 on the first request
// against a user with no manager, which every gate had passed.
type ExpansionOf<D> = NonNullable<D> extends readonly unknown[]
  ? RelationshipExpansion[]
  : RelationshipExpansion | null;

/**
 * A requested member as the store actually returns it: the key is ALWAYS
 * PRESENT, holding `null` when the record has no value for it (verified
 * 2026-08-18 with a `fields` read of a user lacking `telephoneNumber`,
 * `description` and `manager` — all three came back `null`, not absent).
 *
 * So a projected member is a REQUIRED key with a NULLABLE value — but only where
 * the schema leaves the property optional, since a schema-required property
 * always has a value. `Pick` was wrong twice here: it kept the `?`, implying the
 * key might be missing, and it kept the value non-null, which is the shape that
 * cost a live 500 on `manager`.
 */
type SelectedMembers<T, F extends string> = {
  [K in Extract<F, keyof T>]-?: undefined extends T[K]
    ? NonNullable<T[K]> | null
    : T[K];
};

// The record shape a `fields` selector actually yields.
//
// `_id` and `_rev` come back whatever you asked for — even `fields: ["_id"]`
// returns both (verified 2026-08-17, docs/api/10-managed-objects.md) — so a
// projection keeps the StoredRecord guarantee. A relationship path upgrades the
// parent key from its declared `RelationshipRef` to the expansion above.
//
// A list that is not made of LITERAL types cannot be projected: a
// `ManagedField<T>[]` variable widens to the whole record rather than narrowing
// to nothing, so losing the inference costs precision and never correctness. (A
// plain `string[]` is rejected outright — the constraint wants schema members.)
type Projected<T, F extends string> = string extends F
  ? StoredRecord<T>
  : StoredRecord<
      ("*" extends F ? T : SelectedMembers<T, F>) & {
        [K in Extract<PathParentOf<F>, keyof T>]: ExpansionOf<T[K]>;
      } & MetaMemberOf<F>
    >;

// Parameters a managed-object query honours. `_queryFilter` stays required —
// IDM rejects a query without one. `_fields`, `_sortKeys` and the paging cursor
// are all exercised against the live tenant in `docs/api/10-managed-objects.md`;
// note that offset paging is deliberately not named, because deep offsets can
// skip or duplicate records and the cursor is the supported walk.
//
// The index signature is deliberate. An exact object type here is precisely why
// `_fields` was a type error rather than a supported parameter, and a misspelled
// `_queryFilter` is still caught by the required key.
interface QueryParams {
  _queryFilter: string;
  _fields?: string;
  _sortKeys?: string;
  _pageSize?: number;
  /**
   * OMIT this key to start at the first page — do NOT pass `null`. IDM
   * rejects an explicit null with
   * `JsonValueException: /_pagedResultsCookie: Expecting a value`
   * (verified 2026-08-18), which surfaces as an opaque 500. That is why
   * neither this key nor the index signature admits `null`: the type used
   * to, and a `cursor ?? null` in a handler was a live 500.
   */
  _pagedResultsCookie?: string;
  [param: string]: string | number | undefined;
}

interface OpenIdm {
  read<R extends `${ManagedName}/${string}` | (string & {})>(
    path: R,
    params?: Record<string, string> | null
  ): [ManagedRecordOf<R>] extends [never]
    ? any
    : StoredRecord<ManagedRecordOf<R>> | null;
  // Restricting the fields narrows the RESULT TYPE to what you asked for, and
  // keeps `_id`/`_rev`, which come back regardless (verified 2026-08-17).
  read<
    R extends `${ManagedName}/${string}` | (string & {}),
    const F extends FieldsArg<ManagedRecordOf<R>, R>
  >(
    path: R,
    params: Record<string, string> | null | undefined,
    fields: F
  ): [ManagedRecordOf<R>] extends [never]
    ? any
    : Projected<ManagedRecordOf<R>, F[number] & string> | null;
  // Rows carry `_id`/`_rev` plus every declared property — unless `params` names
  // `_fields`, which trims them opaquely; prefer the third argument below.
  query<R extends ManagedName | (string & {}), P extends QueryParams>(
    path: R,
    params: P
  ): [ManagedCollectionOf<R>] extends [never]
    ? any
    : QueryResult<
        P extends { _fields: string }
          ? StoredRecord<Partial<ManagedCollectionOf<R>>>
          : StoredRecord<ManagedCollectionOf<R>>
      >;
  // `openidm.query` takes a third `fields` argument, same as `read` — verified
  // 2026-08-17: rows came back as `_id`/`_rev` plus exactly the named fields.
  query<
    R extends ManagedName | (string & {}),
    const F extends FieldsArg<ManagedCollectionOf<R>, R>
  >(
    path: R,
    params: QueryParams,
    fields: F
  ): [ManagedCollectionOf<R>] extends [never]
    ? any
    : QueryResult<Projected<ManagedCollectionOf<R>, F[number] & string>>;
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
