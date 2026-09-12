/** JSON values the runner protocol can carry (job globals and completion values). */
export type JsonValue =
  | null
  | boolean
  | number
  | string
  | JsonValue[]
  | { [key: string]: JsonValue };

/**
 * Machine-readable job outcomes. These are different bugs; the harness must
 * not collapse them into one string.
 *
 * - `ok` — compiled and ran
 * - `compile_error` — failed to compile
 * - `runtime_error` — threw at runtime
 * - `timeout` — instruction observer fired (`Error("Interrupt.")`)
 * - `protocol_error` — the job itself was malformed
 */
export type JobOutcome =
  | "ok"
  | "compile_error"
  | "runtime_error"
  | "timeout"
  | "protocol_error";

export interface JobRequest {
  /** Correlates the response. Generated if omitted. */
  id?: string;
  source: string;
  /** Author's file path; becomes Rhino's source name (not `<eval>`). */
  sourceName?: string;
  /** Rhino language version. Default 0 (`VERSION_DEFAULT`). */
  languageVersion?: number | string;
  /**
   * Per-job timeout in milliseconds. `0` means no timeout (AM's default).
   * Omitted uses the runner's harness default (10s) so a runaway cannot hang
   * the process.
   */
  timeoutMs?: number;
  /** JSON values placed in ENGINE_SCOPE Bindings before eval. */
  globals?: { [key: string]: JsonValue };
  /**
   * Java class allow-list for this job, as a script context's `allowLists`
   * entries (exact names, or a trailing `*` meaning prefix). Supplying one
   * installs AM's class shutter; omitting it leaves every Java name resolvable,
   * which is the pre-shutter behaviour.
   */
  classAllowList?: string[];
  /** Evaluated first, so author line numbers on `source` stay intact. */
  preamble?: string;
  preambleName?: string;
}

export interface JobError {
  class: string;
  message: string | null;
  sourceName: string | null;
  line: number;
  column: number;
  lineSource: string | null;
}

export interface JobResponse {
  id: string;
  outcome: JobOutcome;
  valueKind: string;
  value: JsonValue;
  error: JobError | null;
}

const OUTCOMES: ReadonlySet<string> = new Set([
  "ok",
  "compile_error",
  "runtime_error",
  "timeout",
  "protocol_error",
]);

export function parseJobResponse(raw: unknown, line: string): JobResponse {
  if (!isRecord(raw)) {
    throw new Error(`rhino-local: runner response is not an object: ${line}`);
  }
  if (typeof raw.id !== "string" || raw.id.length === 0) {
    throw new Error(`rhino-local: runner response has no id: ${line}`);
  }
  if (typeof raw.outcome !== "string" || !OUTCOMES.has(raw.outcome)) {
    throw new Error(`rhino-local: runner response has unknown outcome: ${line}`);
  }
  if (typeof raw.valueKind !== "string") {
    throw new Error(`rhino-local: runner response is missing valueKind: ${line}`);
  }
  return {
    id: raw.id,
    outcome: raw.outcome as JobOutcome,
    valueKind: raw.valueKind,
    value: raw.value as JsonValue,
    error: parseJobError(raw.error),
  };
}

function parseJobError(raw: unknown): JobError | null {
  if (raw === null || raw === undefined) {
    return null;
  }
  if (!isRecord(raw)) {
    throw new Error("rhino-local: runner error is not an object");
  }
  if (typeof raw.class !== "string") {
    throw new Error("rhino-local: runner error is missing class");
  }
  return {
    class: raw.class,
    message: optionalString(raw.message),
    sourceName: optionalString(raw.sourceName),
    line: optionalNumber(raw.line, -1),
    column: optionalNumber(raw.column, -1),
    lineSource: optionalString(raw.lineSource),
  };
}

function optionalString(raw: unknown): string | null {
  if (raw === null || raw === undefined) {
    return null;
  }
  if (typeof raw !== "string") {
    throw new Error("rhino-local: expected string or null");
  }
  return raw;
}

function optionalNumber(raw: unknown, fallback: number): number {
  if (raw === null || raw === undefined) {
    return fallback;
  }
  if (typeof raw !== "number" || !Number.isFinite(raw)) {
    throw new Error("rhino-local: expected finite number");
  }
  return raw;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
