/** CREST response and host error projection; throw policy is context-dependent. */
import type { JobResponse, JsonValue } from "../protocol.ts";
import { HostCallError } from "../runner.ts";
import type { Evidence } from "./provenance.ts";
import type { Document } from "./types.ts";

const reasons: Record<number, string> = {
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

  constructor(status: number, message: string, detail?: JsonValue, rawBody?: string) {
    super(message);
    this.status = status;
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

export function missing(evidence: Evidence, behavior: string): never {
  evidence.add("missing", behavior);
  throw new IdmError(501, `Missing IDM behavior: ${behavior}`);
}

export function scriptError(response: JobResponse, context: string, evidence: Evidence): never {
  const thrown = response.thrown;
  const endpoint = context === "endpoint";
  const diagnostic = response.error?.message ?? "Internal Server Error";
  if (diagnostic.includes("is not emulated yet")) missing(evidence, diagnostic);
  if (thrown?.kind === "object" && typeof thrown.code === "number") {
    if (endpoint || context === "onCreate") evidence.measured(`${context}: numeric object throw`);
    else evidence.assumed(`${context}: numeric object throw uses CREST status`);
    const message = typeof thrown.message === "string" ? thrown.message : "[object Object]";
    throw new IdmError(thrown.code, message, thrown.detail ?? {});
  }
  if (endpoint && thrown?.kind === "object" && typeof thrown.code === "string") {
    evidence.measured("endpoint: string code throw yields empty 500");
    throw new IdmError(500, "", undefined, "");
  }
  if (context === "property onValidate") {
    if (thrown?.kind === "string" || thrown?.kind === "error") {
      evidence.measured("property onValidate: string and script errors are forbidden");
    } else evidence.assumed("property onValidate: other throws are forbidden");
    const message = thrown?.kind === "string" ? String(thrown.value) :
      `javax.script.ScriptException: ${String(thrown?.message ?? diagnostic)}`;
    throw new IdmError(403, message);
  }
  if (endpoint && (thrown?.kind === "string" || thrown?.kind === "error")) {
    evidence.measured("endpoint: string and Error throws yield 500 with message");
    throw new IdmError(500, String(thrown.message ?? thrown.value ?? diagnostic));
  }
  if (context === "onCreate" && thrown?.kind === "error") {
    evidence.measured("onCreate: Error throw yields generic 500");
  } else evidence.assumed(`${context}: unmeasured throw yields generic 500`);
  throw new IdmError(500, "Internal Server Error");
}
