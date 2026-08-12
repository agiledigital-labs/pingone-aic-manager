// Next-generation-only common bindings, shared by next-gen scripted decision and
// library scripts (both run on the next-gen engine). Verified ABSENT on the
// legacy engine (2026-06-03), so the legacy decision leaf does NOT include this
// file. Layered on top of rhino + common.
//
// Signatures transcribed from the script editor's binding metadata
// (docs/api/bindings/scripted-decision-next.json, 2026-06-04) — authoritative.

// slf4j-style logger. `{}` placeholders in `format` are filled by the args.
type LogFunction = (message: StringLike, ...args: any[]) => void;
interface Logger {
  getName(): string;
  trace: LogFunction;
  debug: LogFunction;
  info: LogFunction;
  warn: LogFunction;
  error: LogFunction;
  isTraceEnabled(): boolean;
  isDebugEnabled(): boolean;
  isInfoEnabled(): boolean;
  isWarnEnabled(): boolean;
  isErrorEnabled(): boolean;
}
declare const logger: Logger;

// ---- utils ---------------------------------------------------------------

/** SubtleCrypto-like API (utils.crypto.subtle). Byte arrays are number[]. */
interface ScriptSubtle {
  sign(algorithm: StringLike, key: number[], data: number[]): number[];
  sign(algorithmOptions: object, key: number[], data: number[]): number[];
  verify(
    algorithm: StringLike,
    key: number[],
    data: number[],
    signature: number[]
  ): boolean;
  verify(
    algorithmOptions: object,
    key: number[],
    data: number[],
    signature: number[]
  ): boolean;
  digest(algorithm: StringLike, data: number[]): number[];
  encrypt(algorithm: StringLike, key: number[], data: number[]): number[];
  encrypt(algorithmOptions: object, key: number[], data: number[]): number[];
  decrypt(algorithm: StringLike, key: number[], data: number[]): number[];
  decrypt(algorithmOptions: object, key: number[], data: number[]): number[];
  generateKey(algorithm: StringLike): object;
  generateKey(algorithm: object): object;
  deriveKey(
    algorithmName: StringLike,
    baseKey: number[],
    derivedKeyLength: number
  ): number[];
  deriveKey(
    algorithm: object,
    baseKey: number[],
    derivedKeyLength: number
  ): number[];
}

interface ScriptCrypto {
  /** Random RFC-4122 v4 UUID. */
  randomUUID(): string;
  /** Fill the given array with random values and return it. */
  getRandomValues(array: number[]): number[];
  /** Web-Crypto SubtleCrypto-style operations. */
  subtle: ScriptSubtle;
}

interface Base64 {
  /** Decode a base64 string to a UTF-8 string. */
  decode(toDecode: StringLike): string;
  /** Base64-encode a UTF-8 string, e.g. utils.base64.encode("user:pass"). */
  encode(toEncode: StringLike): string;
  /** Base64-encode raw bytes. */
  encode(toEncode: number[]): string;
  /** Decode a base64 string to raw bytes. */
  decodeToBytes(toDecode: StringLike): number[];
  /** Browser-style: encode a binary string to base64. */
  btoa(toEncode: StringLike): string;
  /** Browser-style: decode base64 to a binary string. */
  atob(toDecode: StringLike): string;
}

interface ScriptTypes {
  /** Byte array → UTF-8 string. */
  bytesToString(bytes: number[]): string;
  /** UTF-8 string → byte array. */
  stringToBytes(value: StringLike): number[];
}

interface Utils {
  /** Random values + UUIDs + Web-Crypto subtle. */
  crypto: ScriptCrypto;
  /** Base64 (standard alphabet). */
  base64: Base64;
  /** Base64url (URL-safe alphabet). */
  base64url: Base64;
  /** String ↔ byte conversions. */
  types: ScriptTypes;
}
declare const utils: Utils;

// ---- openidm (CRUDPAQ) ---------------------------------------------------
//
// Resource names are plain strings (e.g. "managed/alpha_user",
// "managed/alpha_user/<id>", "internal/role/<id>"). The generated
// `types/managed/openidm-map.d.ts` merges the tenant's managed objects into
// the empty `interface ManagedObjects {}` below. Single generic signatures +
// conditionals keep field typos pinned to the offending element with spelling
// suggestions, while preserving the loose fallback for non-managed resources.

type Patch =
  | {
      operation: "add" | "replace";
      field: string;
      value: string | string[] | object;
    }
  | {
      operation: "remove";
      field: string;
    };

type QueryResponse = {
  result: Array<{ _id: string; _rev: string } & Record<string, any>>;
  resultCount: number;
  pagedResultsCookie: string | null;
  totalPagedResultsPolicy: string;
  totalPagedResults: number;
  remainingPagedResults: number;
};

// The generated `types/managed/openidm-map.d.ts` fills this with
// collection-path → interface mappings for the tenant's managed objects.
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
    resourceName: R,
    params?: object
  ): [ManagedRecordOf<R>] extends [never]
    ? any
    : StoredRecord<ManagedRecordOf<R>> | null;
  // Restricting the returned fields drops the `_id`/`_rev` guarantee: we have
  // not verified that IDM still includes them, so this form keeps them optional.
  read<R extends `${ManagedName}/${string}` | (string & {})>(
    resourceName: R,
    params: object | undefined,
    fields: FieldsArg<ManagedRecordOf<R>, R>
  ): [ManagedRecordOf<R>] extends [never] ? any : ManagedRecordOf<R> | null;
  create<R extends ManagedName | (string & {})>(
    resourceName: R,
    newResourceId: string | null,
    content: ContentArg<ManagedCollectionOf<R>>,
    params?: object,
    fields?: FieldsArg<ManagedCollectionOf<R>, R>
  ): RecordResult<ManagedCollectionOf<R>>;
  update<R extends `${ManagedName}/${string}` | (string & {})>(
    id: R,
    rev: string | null,
    value: ContentArg<ManagedRecordOf<R>>,
    params?: object,
    fields?: FieldsArg<ManagedRecordOf<R>, R>
  ): RecordResult<ManagedRecordOf<R>>;
  patch<R extends `${ManagedName}/${string}` | (string & {})>(
    resourceName: R,
    rev: string | null,
    patch: Patch[],
    params?: object,
    fields?: FieldsArg<ManagedRecordOf<R>, R>
  ): RecordResult<ManagedRecordOf<R>>;
  delete<R extends `${ManagedName}/${string}` | (string & {})>(
    resourceName: R,
    rev: string | null,
    params?: object,
    fields?: FieldsArg<ManagedRecordOf<R>, R>
  ): RecordResult<ManagedRecordOf<R>>;
  query<R extends ManagedName | (string & {})>(
    resourceName: R,
    params: { _queryFilter: string } | object,
    fields?: FieldsArg<ManagedCollectionOf<R>, R>
  ): [ManagedCollectionOf<R>] extends [never]
    ? QueryResponse
    : QueryResult<ManagedCollectionOf<R>>;
  action(
    resource: string,
    actionName: string,
    content?: object,
    params?: object,
    fields?: string[]
  ): any;
}
declare const openidm: OpenIdm;

// ---- other next-gen-common bindings (present in every next-gen context) ----

/** Name of the current request cookie. */
declare const cookieName: string;

/** Generate a signed JWT assertion from the given claims. */
interface JwtAssertion {
  generateJwt(jwtData: object): string;
}
declare const jwtAssertion: JwtAssertion;

/** Validate a JWT's claims. */
interface JwtValidator {
  validateJwtClaims(jwtData: object): object;
}
declare const jwtValidator: JwtValidator;

/** Evaluate policies via the policy engine. */
interface Policy {
  evaluate(
    subject: object,
    application: string,
    resourceNames: string[],
    environment: object
  ): any[];
}
declare const policy: Policy;

// ---- managed-user attribute names (for the identity binding) ----------------
//
// AM-side attribute names for managed-user profile data, used by
// `identity`/`idRepository` getters (e.g. `getAttributeValues(<name>)`). These
// differ from the IDM property names — see docs/api/14-am-identity-attributes.md
// for the full IDM→AM mapping and what's verified. Used to give next-gen
// identity bindings autocomplete on the attribute name (the value always comes
// back as a string array, so only the name is typed). NOT exhaustive of every
// possible LDAP attribute — a `string` fallback overload stays for the rest.
//
// Note: relationship-typed attributes (manager, fr-idm-managed-user-manager,
// *-roles, *-member, …) do NOT surface via the scripted-decision
// `getAttributeValues` binding (verified) — kept here for other contexts.
type AmUserAttribute =
  | "uid"
  | "cn"
  | "givenName"
  | "sn"
  | "mail"
  | "displayName"
  | "description"
  | "userPassword"
  | "telephoneNumber"
  | "street"
  | "l"
  | "st"
  | "postalCode"
  | "co"
  | "inetUserStatus"
  | "iplanet-am-user-alias-list"
  | "assignedDashboard"
  | "labeledURI"
  | "pwdChangedTime"
  | "pwdExpirationTime"
  | "dn"
  | "fr-idm-uuid"
  | "etag"
  | "fr-idm-custom-attrs"
  | "fr-idm-kbaInfo"
  | "fr-idm-preferences"
  | "fr-idm-consentedMapping"
  | "fr-idm-managed-user-meta"
  | "fr-idm-managed-user-manager"
  | "manager"
  | "fr-idm-managed-user-roles"
  | "fr-idm-managed-user-groups"
  | "fr-idm-managed-application-member"
  | "fr-idm-managed-application-owner"
  | "fr-idm-managed-assignment-member"
  | "fr-idm-effectiveAssignment"
  | "fr-idm-effectiveApplications"
  | "fr-idm-effectiveGroup"
  | "fr-idm-effectiveRole"
  | "fr-idm-managed-organization-admin"
  | "fr-idm-managed-organization-owner"
  | "fr-idm-managed-organization-member"
  | "fr-idm-managed-user-memberoforgid"
  | "fr-idm-managed-user-task-principals"
  | "fr-idm-managed-user-notifications"
  | "fr-attr-istr1"
  | "fr-attr-istr2"
  | "fr-attr-istr3"
  | "fr-attr-istr4"
  | "fr-attr-istr5"
  | "fr-attr-istr6"
  | "fr-attr-istr7"
  | "fr-attr-istr8"
  | "fr-attr-istr9"
  | "fr-attr-istr10"
  | "fr-attr-istr11"
  | "fr-attr-istr12"
  | "fr-attr-istr13"
  | "fr-attr-istr14"
  | "fr-attr-istr15"
  | "fr-attr-istr16"
  | "fr-attr-istr17"
  | "fr-attr-istr18"
  | "fr-attr-istr19"
  | "fr-attr-istr20"
  | "fr-attr-str1"
  | "fr-attr-str2"
  | "fr-attr-str3"
  | "fr-attr-str4"
  | "fr-attr-str5"
  | "fr-attr-imulti1"
  | "fr-attr-imulti2"
  | "fr-attr-imulti3"
  | "fr-attr-imulti4"
  | "fr-attr-imulti5"
  | "fr-attr-multi1"
  | "fr-attr-multi2"
  | "fr-attr-multi3"
  | "fr-attr-multi4"
  | "fr-attr-multi5"
  | "fr-attr-idate1"
  | "fr-attr-idate2"
  | "fr-attr-idate3"
  | "fr-attr-idate4"
  | "fr-attr-idate5"
  | "fr-attr-date1"
  | "fr-attr-date2"
  | "fr-attr-date3"
  | "fr-attr-date4"
  | "fr-attr-date5"
  | "fr-attr-iint1"
  | "fr-attr-iint2"
  | "fr-attr-iint3"
  | "fr-attr-iint4"
  | "fr-attr-iint5"
  | "fr-attr-int1"
  | "fr-attr-int2"
  | "fr-attr-int3"
  | "fr-attr-int4"
  | "fr-attr-int5"
  | "deviceProfiles"
  | "devicePrintProfiles"
  | "webauthnDeviceProfiles"
  | "oathDeviceProfiles"
  | "pushDeviceProfiles";

// Only next-generation scripts can require() library scripts (resolved via the
// leaf tsconfig `paths` alias to ../lib/*). `require` is a module mechanism, not
// a listed binding, so it's shared here across all next-gen contexts.
declare function require(id: string): any;
