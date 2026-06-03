// Bindings present in BOTH the next-generation and legacy AM scripted-decision
// engines (verified 2026-06-03 — see docs/api/12-script-bindings-matrix.md):
//   logger, httpClient, systemEnv, realm, scriptName.
// Next-gen-only common bindings (openidm, utils) live in nextgen-common.d.ts so
// the legacy leaf doesn't falsely see them. Shapes here target next-gen; legacy
// shape differences for the shared bindings (logger/httpClient) are a deferred
// probe, noted in decision-node-legacy.d.ts.

declare const scriptName: string;
declare const realm: string;

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
