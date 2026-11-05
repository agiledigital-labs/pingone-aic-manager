import { EmulationFailure, responseFailure } from "../emulation.ts";
/** CREST response and host error projection; throw policy is context-dependent. */
import type { JobResponse, JsonValue } from "../protocol.ts";
import { HostCallError } from "../runner.ts";
import type { Evidence } from "./provenance.ts";
import type { Document } from "./types.ts";

/** The HTTP error serializer escapes strings, including nested detail members. */
export function escapeError(value: JsonValue): JsonValue {
  if (typeof value === "string") {
    const entities: Record<string, string> = {
      "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&#34;", "'": "&#39;",
      "=": "&#61;", "`": "&#96;", "@": "&#64;",
    };
    return value.replace(/[&<>"'=`@]/g, (character) => entities[character] ?? character);
  }
  if (Array.isArray(value)) return value.map(escapeError);
  if (value !== null && typeof value === "object") return Object.fromEntries(Object.entries(value).map(([key, child]) => [key, escapeError(child)]));
  return value;
}

/** CREST's reason phrase per status; the script tester's runtime.cjs holds the same table (`__rhinoLocalCrestReasons`). */
export const reasons: Record<number, string> = {
  400: "Bad Request", 401: "Unauthorized", 403: "Forbidden", 404: "Not Found",
  405: "Method Not Allowed", 409: "Conflict", 412: "Precondition Failed",
  500: "Internal Server Error", 501: "Not Implemented", 503: "Service Unavailable",
};
const classes: Record<number, string> = {
  400: "BadRequestException", 401: "UnauthorizedException", 403: "ForbiddenException",
  404: "NotFoundException", 405: "NotSupportedException", 409: "ConflictException",
  412: "PreconditionFailedException", 500: "InternalServerErrorException",
  501: "NotSupportedException", 503: "ServiceUnavailableException",
};

export class IdmError extends Error {
  readonly status: number;
  readonly body: Document;
  readonly rawBody?: string;
  readonly escapeHttp: boolean;
  readonly assumption: string | undefined;

  constructor(status: number, message: string, detail?: JsonValue, rawBody?: string, escapeHttp = false, assumption?: string) {
    super(message);
    this.status = status;
    this.escapeHttp = escapeHttp;
    this.assumption = assumption;
    this.body = { code: status, reason: reasons[status] ?? "Resource Exception", message,
      ...(detail === undefined ? {} : { detail }) };
    if (rawBody !== undefined) this.rawBody = rawBody;
  }

  host(): HostCallError {
    return new HostCallError({
      class: `org.forgerock.json.resource.${classes[this.status] ?? "ResourceException"}`,
      message: this.message, code: this.status, detail: this.body.detail ?? {},
    });
  }
}

export function invalidInput(message: string): IdmError {
  return new IdmError(400, message, undefined, undefined, false, "invalid request shape uses local 400 status and diagnostic");
}

/**
 * `id`, when given, is the harness-fault id the tally ranks this refusal under (E19); the cause then
 * reads `<id>: <behavior>`, as the AM runner's does, so `expectHarnessFault(id)` matches it.
 */
export function missing(evidence: Evidence, behavior: string, id?: string): never {
  const cause = id ? `${id}: ${behavior}` : behavior;
  evidence.add("missing", cause);
  throw new EmulationFailure({ kind: "missing", message: `Missing IDM behavior: ${cause}`, missing: [cause],
    ...(id ? { faults: [{ id, message: behavior }] } : {}) });
}

export function scriptError(response: JobResponse, context: string, evidence: Evidence): never {
  const fatal = responseFailure(response);
  if (fatal) throw new EmulationFailure(fatal);
  const thrown = response.thrown;
  const endpoint = context === "endpoint";
  const failure = (status: number, message: string, detail?: JsonValue, rawBody?: string) =>
    new IdmError(status, message, detail, rawBody, endpoint);
  const diagnostic = response.error?.message ?? "Internal Server Error";
  if (endpoint && response.outcome === "compile_error") {
    evidence.measured("endpoint with invalid source is unroutable", "docs/api/11-idm-endpoints.md");
    throw new IdmError(404, "Not Found");
  }
  if (thrown?.kind === "object" && typeof thrown.code === "number") {
    if (endpoint || context === "onCreate") evidence.measured(`${context}: numeric object throw`);
    else evidence.assumed(`${context}: numeric object throw uses CREST status`);
    const message = typeof thrown.message === "string" ? thrown.message : "[object Object]";
    throw failure(thrown.code, message, thrown.detail ?? {});
  }
  if (endpoint && thrown?.kind === "object" && typeof thrown.code === "string") {
    evidence.measured("endpoint: string code throw yields empty 500");
    throw failure(500, "", undefined, "");
  }
  if (context === "property onValidate") {
    if (thrown?.kind === "string" || (thrown?.kind === "error" && String(thrown.message).startsWith("ReferenceError:"))) {
      evidence.measured("property onValidate: string and script errors are forbidden");
    } else evidence.assumed("property onValidate: other throws are forbidden");
    const message = thrown?.kind === "string" ? String(thrown.value) :
      `javax.script.ScriptException: ${String(thrown?.message ?? diagnostic)}`;
    throw failure(403, message);
  }
  if (endpoint && (thrown?.kind === "string" || thrown?.kind === "error")) {
    if (thrown.kind === "string" || String(thrown.message).startsWith("Error:")) {
      evidence.measured("endpoint: string and Error throws yield 500 with message");
    } else evidence.assumed("endpoint: other script errors yield 500 with message");
    throw failure(500, String(thrown.message ?? thrown.value ?? diagnostic));
  }
  if (thrown?.kind === "javaException" && typeof thrown.code === "number") {
    evidence.assumed(`${context}: host CREST exception preserves status and detail`);
    throw failure(thrown.code, String(thrown.message ?? diagnostic), thrown.detail ?? {});
  }
  if (context === "onCreate" && thrown?.kind === "error" && String(thrown.message).startsWith("Error:")) {
    evidence.measured("onCreate: Error throw yields generic 500");
  } else evidence.assumed(`${context}: unmeasured throw yields generic 500`);
  throw failure(500, "Internal Server Error");
}
