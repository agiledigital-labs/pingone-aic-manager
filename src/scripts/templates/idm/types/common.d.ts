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
// Always guard `context.http` before use. Other contexts (security, oauth2,
// transactionId, …) vary, hence the index signature.
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
  [key: string]: any;
}
declare const context: IdmContext;

declare const identityServer: {
  getProperty(name: string, defaultValue?: string, substitute?: boolean): string | null;
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

interface OpenIdm {
  read<R extends `${ManagedName}/${string}` | (string & {})>(
    path: R,
    params?: Record<string, string> | null,
    fields?: FieldsArg<ManagedRecordOf<R>, R>
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
