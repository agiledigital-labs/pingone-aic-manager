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

interface Crypto {
  randomUUID(): string;
}
interface Utils {
  crypto: Crypto;
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
