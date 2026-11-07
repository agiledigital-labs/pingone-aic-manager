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
 * - `timeout` — harness deadline expired (instruction observer or host wait)
 * - `protocol_error` — the job itself was malformed
 */
export type JobOutcome = "ok" | "compile_error" | "runtime_error" | "timeout" | "protocol_error";

export interface JobRequest {
  /** Preserve internal completion/binding numbers separately from endpoint wire output. */
  internalValues?: boolean;
  /** Correlates the response. Generated if omitted. */
  id?: string;
  source: string;
  /** AM is the default; IDM uses the measured compiled engine and Java bindings. */
  engine?: "am" | "idm";
  /** Opt-in host transport for the journey AM evaluator. */
  hostCalls?: boolean;
  /** IDM top-level bindings, wrapped as Java-backed maps/lists. */
  bindings?: { [key: string]: JsonValue };
  /** IDM aliases resolved to the same backing object before script evaluation. */
  bindingAliases?: Record<string, string>;
  /** IDM environment property snapshot (for identityServer.getProperty). */
  properties?: Record<string, JsonValue>;
  /** Binding names to wrap as Java resource-name objects instead of JS strings. */
  resourceNameBindings?: string[];
  /** Also include an empty missing list when no unsupported IDM behavior was reached. */
  reportMissing?: boolean;
  /** Wrap request/context bindings with CREST request tags and HTTP header lookup. */
  crestBindings?: boolean;
  /**
   * IDM only: `require(id)` asks the host for the module's source with a
   * `require` host call, instead of refusing as missing `require`. The IDM
   * emulator sets it; it answers from its runtime-script provider (E15).
   */
  idmRequire?: boolean;
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
  /**
   * Evaluated before `preamble`, in the same scope and under the same
   * `preambleName`; `preamble`'s line numbers continue from it, so the two
   * behave as `prelude + preamble` sent as one string would. Unlike
   * `preamble` it is shipped to a JVM once and compiled once, then reused by
   * every job that sends the same text — so it is for the part of a preamble
   * that does not change between jobs. Each job still executes it into its
   * own fresh scope.
   */
  prelude?: string;
  /**
   * Text visible to the preamble (and prelude) as the string global
   * `__rhinoLocalPreambleData`, deleted before `source` runs. Lets per-job
   * data reach the preamble as data rather than as source to compile.
   */
  preambleData?: string;
  /**
   * `require()` bodies by id, for the rhino-local runtime's loader. Each
   * distinct body is shipped to a JVM once and compiled once; a body that
   * changes is a different body. Visible to the preamble only.
   */
  libraries?: Record<string, string>;
  /** Return this global after evaluation instead of Rhino's completion value. */
  resultGlobal?: string;
}

export interface JobError {
  class: string;
  message: string | null;
  sourceName: string | null;
  line: number;
  column: number;
  lineSource: string | null;
}

/** `require` is the IDM engine's module load, sent only by jobs with `idmRequire` (E15). */
export type HostMethod = "read" | "create" | "update" | "patch" | "delete" | "query" | "action" | "http" | "require";

export interface HostCall {
  type: "call";
  id: string;
  /** One-based, per job. */
  seq: number;
  method: HostMethod;
  /** Functions are removed; a trailing query callback executes on the JVM. */
  args: JsonValue[];
}

export interface EmulationFailureInfo {
  kind: "missing" | "protocol" | "timeout" | "internal";
  message: string;
  missing?: string[];
  /**
   * Harness faults (E19): refusals of behaviour the harness does not emulate,
   * each under a stable id. Present when the refusal came from a site that has
   * one; the run tally (src/harness/fault-tally.ts) counts by id.
   */
  faults?: HarnessFault[];
}

export interface HarnessFault {
  /** Stable kebab slug, e.g. `script-defect/send-body-js-date`, `arity/openidm.read`, `fixture/http`. */
  id: string;
  message: string;
}

export interface HostError {
  emulation?: EmulationFailureInfo;
  /** Fidelity failures remain visible even when the script catches this error. */
  missing?: string[];
  class: string;
  message: string;
  code: number;
  /** CREST's reason phrase, for a script's `getReason()`; absent, that call faults. */
  reason?: string;
  detail: JsonValue;
}

export interface BehaviorEvidence {
  kind: "measured" | "assumed" | "missing";
  behavior: string;
  source?: string;
}

export type HostReply = (
  | { type: "reply"; id: string; seq: number; result: JsonValue }
  | { type: "reply"; id: string; seq: number; error: HostError }
) & { provenance?: BehaviorEvidence[] };

export interface JobLog {
  surface: "logger" | "console";
  level: string;
  message: string;
  args: JsonValue[];
}

/** Raw throw classification; the emulator applies hook/endpoint-specific HTTP policy. */
export interface ThrownValue {
  kind: "object" | "error" | "string" | "primitive" | "javaException" | "java";
  class?: string;
  value?: JsonValue;
  code?: JsonValue;
  message?: JsonValue;
  detail?: JsonValue;
}

export interface JobResponse {
  /** Evidence on consumed host replies, including original source attribution. */
  provenance?: BehaviorEvidence[];
  internalValue?: JsonValue;
  /** Host sequences emitted but never consumed by the JVM worker. */
  unanswered?: number[];
  emulation?: EmulationFailureInfo;
  id: string;
  outcome: JobOutcome;
  valueKind: string;
  value: JsonValue;
  error: JobError | null;
  /** Present only on IDM responses. Includes mutations to the supplied binding graph. */
  bindings?: { [key: string]: JsonValue };
  logs?: JobLog[];
  thrown?: ThrownValue;
  /** Reached fidelity failures survive script catches; independent of job outcome. */
  missing?: string[];
  /**
   * The harvest of a run the script completed but a harness fault failed: what
   * the script did around the refusal it caught. Diagnostics only — the run has
   * failed, and nothing judges a verdict from this.
   */
  faultedValue?: JsonValue;
  /** Assumptions reached by IDM runtime helpers. */
  assumed?: string[];
  /** Measured behavior reached by IDM runtime helpers, with probe attribution. */
  measured?: string[];
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
    ...(raw.bindings === undefined
      ? {}
      : { bindings: raw.bindings as { [key: string]: JsonValue } }),
    ...(raw.logs === undefined ? {} : { logs: raw.logs as JobLog[] }),
    ...(raw.thrown === undefined ? {} : { thrown: raw.thrown as ThrownValue }),
    ...(raw.missing === undefined ? {} : { missing: raw.missing as string[] }),
    ...(Object.hasOwn(raw, "internalValue") ? { internalValue: raw.internalValue as JsonValue } : {}),
    ...(raw.unanswered === undefined ? {} : { unanswered: raw.unanswered as number[] }),
    ...(raw.emulation === undefined ? {} : { emulation: raw.emulation as EmulationFailureInfo }),
    ...(raw.assumed === undefined ? {} : { assumed: raw.assumed as string[] }),
    ...(raw.measured === undefined ? {} : { measured: raw.measured as string[] }),
    ...(raw.provenance === undefined ? {} : { provenance: raw.provenance as BehaviorEvidence[] }),
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

const HOST_METHODS: ReadonlySet<string> = new Set([
  "read",
  "create",
  "update",
  "patch",
  "delete",
  "query",
  "action",
  "http",
  "require",
]);

export function parseHostCall(raw: unknown): HostCall | undefined {
  if (!isRecord(raw) || raw.type !== "call") return undefined;
  if (
    typeof raw.id !== "string" ||
    raw.id.length === 0 ||
    !Number.isSafeInteger(raw.seq) ||
    (raw.seq as number) < 1 ||
    typeof raw.method !== "string" ||
    !HOST_METHODS.has(raw.method) ||
    !Array.isArray(raw.args)
  ) {
    throw new Error("rhino-local: malformed host call");
  }
  return raw as unknown as HostCall;
}
