// Common bindings for next-generation AM scripts (scripted decision, library,
// and other next-gen families). Shapes target the next-generation engine, which
// is what these workspaces primarily edit.
//
// Verified present in a next-gen scripted decision node (2026-06-03):
//   logger, httpClient, openidm, utils, systemEnv, realm, scriptName.
// The legacy decision overlay (decision-node-legacy.d.ts) adds legacy-only
// globals rather than redeclaring these; legacy shape differences for these
// bindings are a deferred probe (matrix open item #1).

declare const scriptName: string;
declare const realm: string;

interface Crypto {
  randomUUID(): string;
}
interface Utils {
  crypto: Crypto;
}
declare const utils: Utils;

interface SystemEnv {
  getProperty: (key: StringLike) => JavaString | null;
}
declare const systemEnv: SystemEnv;

// Next-gen logger is slf4j-style (trace/debug/info/warn/error). `{}` in the
// message is a placeholder filled by the trailing args.
type LogFunction = (message: StringLike, ...args: any[]) => void;
interface Logger {
  trace: LogFunction;
  debug: LogFunction;
  info: LogFunction;
  warn: LogFunction;
  error: LogFunction;
}
declare const logger: Logger;

interface HttpHeaders {
  "Content-Type"?: "application/json" | "application/x-www-form-urlencoded";
  "X-Api-Key"?: string;
  Authorization?: string;
  x_creation_datetime?: string;
  "x-correlation-id"?: string;
  "x-requesting-system-id"?: string;
}
interface HttpOptions {
  method: "GET" | "POST" | "PUT" | "DELETE";
  clientName?: string;
  headers?: HttpHeaders;
  token?: string;
  body?: object;
  form?: object;
}
interface HttpResponse {
  status: number;
  ok: boolean;
  statusText: string;
  headers: HttpHeaders;
  json(): object;
  text(): string;
}
interface HttpClient {
  send(
    requestUrl: string,
    httpOptions: HttpOptions
  ): {
    get(): HttpResponse;
  };
}
declare const httpClient: HttpClient;

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
