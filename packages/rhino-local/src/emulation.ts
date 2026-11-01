/** Harness/fidelity failures are never tenant CREST exceptions. */
import type { EmulationFailureInfo, JobResponse } from "./protocol.ts";

export class EmulationFailure extends Error {
  readonly emulation: EmulationFailureInfo;
  constructor(failure: EmulationFailureInfo) {
    super(failure.message);
    this.name = "EmulationFailure";
    this.emulation = failure;
  }
}

export function emulationFailure(error: unknown): EmulationFailureInfo | undefined {
  if (error === null || typeof error !== "object") return undefined;
  if (Object.hasOwn(error, "emulation") && "emulation" in error && error.emulation) return error.emulation as EmulationFailureInfo;
  if (Object.hasOwn(error, "missing") && "missing" in error && Array.isArray(error.missing) && error.missing.length > 0) {
    const absent = error.missing as string[];
    return { kind: "missing", message: `Missing IDM behavior: ${absent.join(", ")}`, missing: absent };
  }
  return undefined;
}

export function responseFailure(response: JobResponse): EmulationFailureInfo | undefined {
  return emulationFailure(response) ?? (response.outcome === "timeout"
    ? { kind: "timeout", message: "Rhino harness timeout" }
    : response.outcome === "protocol_error"
      ? { kind: "protocol", message: response.error?.message ?? "IDM protocol failure" } : undefined);
}

export function failedResponse(id: string, failure: EmulationFailureInfo): JobResponse {
  return { id, outcome: failure.kind === "timeout" ? "timeout" : failure.kind === "protocol" ? "protocol_error" : "runtime_error",
    valueKind: "missing", value: null, error: { class: "rhino.local.EmulationFailure", message: failure.message,
      sourceName: null, line: -1, column: -1, lineSource: null }, emulation: failure,
    ...(failure.missing ? { missing: failure.missing } : {}) };
}

/** Framework-free evidence retained independently of any returned status. */
export interface RecordedEmulationFailure {
  readonly sequence: number;
  readonly context: string;
  readonly failure: EmulationFailureInfo;
}

export interface EmulationFailureSource {
  readonly emulationFailures: readonly RecordedEmulationFailure[];
  resetEmulationFailures(): void;
}

/** Append-only until explicitly reset; readers receive detached snapshots. */
export class EmulationFailureJournal {
  #entries: RecordedEmulationFailure[] = [];
  #sequence = 0;

  record(failure: EmulationFailureInfo, context: string): void {
    this.#entries.push({ sequence: ++this.#sequence, context, failure: structuredClone(failure) });
  }

  get entries(): readonly RecordedEmulationFailure[] { return structuredClone(this.#entries); }

  // Keep sequence monotonic across resets so lifecycle checkpoints remain valid.
  reset(): void { this.#entries = []; }
}
