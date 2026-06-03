// Bindings present on BOTH engines with the SAME shape (verified 2026-06-03):
//   systemEnv, realm, scriptName.
// `logger` differs by engine and is NOT here: the next-gen (slf4j) shape lives
// in nextgen-common.d.ts, the classic (Debug) shape in legacy-common.d.ts, so
// each leaf gets exactly one. `httpClient` is present on both but its legacy
// shape is unverified, so it keeps the next-gen shape here (documented imperfection).

declare const scriptName: string;
declare const realm: string;

interface SystemEnv {
  getProperty: (key: StringLike) => JavaString | null;
}
declare const systemEnv: SystemEnv;

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
