// Bindings present on ALL AM leaves — both engine generations and every context
// (verified across the legacy probe + the next-gen context metadata): realm,
// scriptName, httpClient, secrets.
//
// `logger` is NOT here — its shape differs by engine (next-gen slf4j lives in
// nextgen-common.d.ts; classic Debug in legacy-common.d.ts). `systemEnv` is NOT
// here either — it is legacy-only (legacy-common.d.ts). `httpClient`/`secrets`
// use their next-gen shapes; legacy shape differences are documented imperfections.

declare const scriptName: string;
declare const realm: string;

// `systemEnv` is present at RUNTIME on both engines (verified via typeof probe,
// 2026-06-04) but is NOT in the next-gen editor binding metadata — an unlisted
// global. Kept here (available everywhere) rather than removed, so next-gen
// scripts that use it still type-check. Prefer documented bindings where possible.
interface SystemEnv {
  getProperty: (key: StringLike) => JavaString | null;
}
declare const systemEnv: SystemEnv;

interface HttpHeaders {
  "Content-Type"?: "application/json" | "application/x-www-form-urlencoded";
  "X-Api-Key"?: string;
  /**
   * NOT honored by the next-gen httpClient — setting Authorization directly here
   * has no effect. Use `HttpOptions.token` (Bearer) instead. (Basic auth via the
   * next-gen client is an open question — see docs/api/12.)
   */
  Authorization?: string;
  x_creation_datetime?: string;
  "x-correlation-id"?: string;
  "x-requesting-system-id"?: string;
}
interface HttpOptions {
  method: "GET" | "POST" | "PUT" | "DELETE";
  clientName?: string;
  headers?: HttpHeaders;
  /**
   * Bearer token: sent as `Authorization: Bearer <token>`. In next-gen scripts
   * you cannot set the Authorization header directly — set `token` instead.
   * (How to send Basic auth this way is not yet known — see docs/api/12.)
   */
  token?: string;
  /**
   * Next-gen serializes every JS number as a double (`1` becomes `1.0`) and
   * sends `undefined` properties as `null`. Box integers with
   * `java.lang.Integer.valueOf(1)` and use `delete body.field` to omit a key.
   */
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
  send(requestUrl: string, httpOptions: HttpOptions): { get(): HttpResponse };
  send(requestUrl: string): { get(): HttpResponse };
}
declare const httpClient: HttpClient;

// Secrets API. Each accessor returns a secret object (read it via its own
// methods). Method set from the next-gen binding metadata; present on legacy too.
interface Secrets {
  getGenericSecret(secretId: StringLike): object;
  getDecryptionKey(secretId: StringLike): object;
  getEncryptionKey(secretId: StringLike): object;
  getSigningKey(secretId: StringLike): object;
  getVerificationKey(secretId: StringLike): object;
}
declare const secrets: Secrets;
