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
// "managed/alpha_user/<id>", "internal/role/<id>"). Overloads mirror the binding
// metadata: trailing params (`params`, `fields`) are optional.

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

interface OpenIdm {
  read(resourceName: string, params?: object, fields?: string[]): any;
  create(
    resourceName: string,
    newResourceId: string | null,
    content: object,
    params?: object,
    fields?: string[]
  ): any;
  update(
    id: string,
    rev: string | null,
    value: object,
    params?: object,
    fields?: string[]
  ): any;
  patch(
    resourceName: string,
    rev: string | null,
    patch: Patch[],
    params?: object,
    fields?: string[]
  ): any;
  delete(resourceName: string, rev: string | null, params?: object, fields?: string[]): any;
  query(resourceName: string, params: { _queryFilter: string } | object, fields?: string[]): QueryResponse;
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
  | "fr-idm-managed-assignment-member";

// Only next-generation scripts can require() library scripts (resolved via the
// leaf tsconfig `paths` alias to ../lib/*). `require` is a module mechanism, not
// a listed binding, so it's shared here across all next-gen contexts.
declare function require(id: string): any;
