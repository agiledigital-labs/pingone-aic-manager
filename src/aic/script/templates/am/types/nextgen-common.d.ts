// Next-generation-only common bindings, shared by next-gen scripted decision and
// library scripts (both run on the next-gen engine). Verified ABSENT on the
// legacy engine (2026-06-03), so the legacy decision leaf does NOT include this
// file. Layered on top of rhino + common.

// Next-gen logger is slf4j-style (trace/debug/info/warn/error). `{}` in the
// message is a placeholder filled by the trailing args. The legacy engine uses
// a different shape (see legacy-common.d.ts), verified 2026-06-04.
type LogFunction = (message: StringLike, ...args: any[]) => void;
interface Logger {
  trace: LogFunction;
  debug: LogFunction;
  info: LogFunction;
  warn: LogFunction;
  error: LogFunction;
}
declare const logger: Logger;

// `utils` surface enumerated from the live binding (2026-06-04): base64,
// base64url, crypto, types. Method argument/return shapes follow the docs +
// Web-Crypto conventions; crypto.subtle/randomValues exact shapes are unverified
// (see docs/api/12 open item #4).
interface Base64 {
  /** Base64-encode a UTF-8 string. e.g. utils.base64.encode("user:pass"). */
  encode(value: StringLike): string;
  /** Decode a base64 string back to a UTF-8 string. */
  decode(value: StringLike): string;
  /** Decode a base64 string to raw bytes. */
  decodeToBytes(value: StringLike): JavaByteArray;
  /** Browser-style: encode a binary string to base64. */
  btoa(value: StringLike): string;
  /** Browser-style: decode base64 to a binary string. */
  atob(value: StringLike): string;
}
interface Crypto {
  /** Random RFC-4122 v4 UUID. */
  randomUUID(): string;
  /** Web-Crypto-style random values (shape unverified — docs/api/12 #4). */
  randomValues(...args: any[]): any;
  getRandomValues(...args: any[]): any;
  /** SubtleCrypto-like API (shape unverified — docs/api/12 #4). */
  subtle: any;
}
interface Types {
  /** UTF-8 string → byte array. */
  stringToBytes(value: StringLike): JavaByteArray;
  /** Byte array → UTF-8 string. */
  bytesToString(bytes: JavaByteArray): string;
}
interface Utils {
  /** Base64 (standard alphabet). */
  base64: Base64;
  /** Base64url (URL-safe alphabet). */
  base64url: Base64;
  /** Random values + UUIDs. */
  crypto: Crypto;
  /** String ↔ byte conversions. */
  types: Types;
}
declare const utils: Utils;

// openidm CRUDPAQ. Tenant-specific managed-object paths keep the editor honest
// about which resources exist.
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

type IdmManagedObject =
  | "alpha_user"
  | "alpha_organization"
  | "providerProvisioningQueue"
  | "bravo_user"
  | "bravo_organization";

type IdmObjectPath = `managed/${IdmManagedObject}`;

type QueryResponse = {
  result: [
    {
      _id: string;
      _rev: string;
    } & object,
  ];
  resultCount: number;
  pagedResultsCookie: string | null;
  totalPagedResultsPolicy: string;
  totalPagedResults: number;
  remainingPagedResults: number;
};

declare const openidm: {
  read: (
    path: `${IdmObjectPath}/${string}`,
    params?: Record<string, string> | null,
    fields?: string[]
  ) => object | null;
  query: (
    path: IdmObjectPath,
    params: { _queryFilter: string },
    fields?: string[]
  ) => QueryResponse;
  create: (
    path: IdmObjectPath,
    newResourceId: string | null,
    content: Record<string, any> | null,
    params?: Record<string, string> | null,
    fields?: string[]
  ) => object;
  patch: (
    path: `${IdmObjectPath}/${string}`,
    revision: string | null,
    patch: Patch[],
    params?: Record<string, string> | null,
    fields?: string[]
  ) => object;
  delete: (
    path: `${IdmObjectPath}/${string}`,
    revision: string | null,
    params?: Record<string, string> | null,
    fields?: string[]
  ) => object;
};
