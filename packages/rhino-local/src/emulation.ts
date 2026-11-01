/** Harness/fidelity failures are never tenant CREST exceptions. */
import { AsyncLocalStorage } from "node:async_hooks";
import { randomUUID } from "node:crypto";
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

/** Framework-free, process-wide audit history and logical work ownership. */
export interface EmulationOwner {
  readonly id: string;
  readonly name: string;
  readonly kind: "test" | "suite";
}
export interface RecordedEmulationFailure {
  readonly sequence: number;
  readonly context: string;
  readonly failure: EmulationFailureInfo;
  readonly owner?: EmulationOwner;
  readonly expected?: string;
}
export interface EmulationFailureSource {
  readonly emulationFailures: readonly RecordedEmulationFailure[];
}
interface Expectation { name: string; match: (cause: string) => boolean; cause?: string | undefined }
interface OwnerState {
  owner: EmulationOwner;
  parent: string | undefined;
  expectations: Expectation[];
  closed: boolean;
  failed: boolean;
  late: RecordedEmulationFailure[];
}
interface Entry extends RecordedEmulationFailure { journal: number }
interface Work { journal: EmulationFailureJournal; owner: EmulationOwner | undefined; context: string }
interface Registry {
  ownership: AsyncLocalStorage<EmulationOwner | undefined>;
  owners: Map<string, OwnerState>;
  history: Entry[];
  outstanding: Set<Work>;
  nextJournal: number;
}
// Share across module graphs too (e.g. a setup file and a separately loaded core).
const registryKey = Symbol.for("@agiledigital/pingone-aic-script-tester/emulation-registry-v1");
const processGlobal = globalThis as typeof globalThis & { [registryKey]?: Registry };
const registry = processGlobal[registryKey] ??= {
  ownership: new AsyncLocalStorage<EmulationOwner | undefined>(), owners: new Map(),
  history: [], outstanding: new Set(), nextJournal: 0,
};
const { ownership, owners, history, outstanding } = registry;

export function currentEmulationOwner(): EmulationOwner | undefined { return ownership.getStore(); }
export function createEmulationOwner(name: string, kind: EmulationOwner["kind"]): EmulationOwner {
  const owner = Object.freeze({ id: randomUUID(), name, kind });
  owners.set(owner.id, { owner, parent: ownership.getStore()?.id, expectations: [], closed: false, failed: false, late: [] });
  return owner;
}
export function withEmulationOwner<T>(owner: EmulationOwner, action: () => T): T { return ownership.run(owner, action); }

/** Event-driven host callbacks must retain the owner captured when eval began. */
export function bindEmulationOwner<A extends unknown[], T>(action: (...args: A) => T): (...args: A) => T {
  const owner = ownership.getStore();
  return (...args) => ownership.run(owner, () => action(...args));
}

/** A wrapper's provenance prefix does not introduce a second underlying cause. */
function causes(failure: EmulationFailureInfo): string[] {
  const missing = (failure.missing ?? []).map((name) => name.replace(/^runtime helper: /, ""));
  return [...new Set(failure.kind === "missing" && missing.length ? missing : [...missing, failure.message])];
}
function coverage(owner: EmulationOwner | undefined): Map<string, string> {
  const claims = owner ? owners.get(owner.id)?.expectations ?? [] : [];
  const available = [...new Set(history.filter((entry) => entry.owner?.id === owner?.id).flatMap((entry) => causes(entry.failure)))];
  const covered = new Map<string, string>();
  for (const claim of claims) {
    // Bind once: one declaration can cover repeated observations of ONE cause.
    claim.cause ??= available.find((cause) => !covered.has(cause) && claim.match(cause));
    if (claim.cause !== undefined) covered.set(claim.cause, claim.name);
  }
  return covered;
}
function unexpected(entry: RecordedEmulationFailure): string[] {
  const covered = coverage(entry.owner);
  return causes(entry.failure).filter((cause) => !covered.has(cause));
}
function snapshot(entry: Entry): RecordedEmulationFailure {
  const covered = coverage(entry.owner);
  const names = causes(entry.failure).map((cause) => covered.get(cause));
  return structuredClone({ sequence: entry.sequence, context: entry.context, failure: entry.failure,
    ...(entry.owner ? { owner: entry.owner } : {}), ...(names.every((name) => name !== undefined) ? { expected: [...new Set(names)].join(", ") } : {}) });
}
export function recordedEmulationFailures(): readonly RecordedEmulationFailure[] { return history.map(snapshot); }

/** Each declaration covers one whole cause; regexes must match its entire name. */
export function expectOwnedEmulationFailure(name: string | RegExp): void {
  const owner = ownership.getStore();
  const state = owner && owners.get(owner.id);
  if (!state || state.closed || owner?.kind !== "test") throw new Error("expectEmulationFailure requires the Vitest emulation setup and an active test");
  const match = (cause: string) => {
    if (typeof name === "string") return cause === name;
    name.lastIndex = 0;
    const result = name.exec(cause);
    return result?.index === 0 && result[0].length === cause.length;
  };
  state.expectations.push({ name: String(name), match });
}

/** No deletion API: source replacement and fork reuse cannot erase audit entries. */
export class EmulationFailureJournal {
  readonly #id = ++registry.nextJournal;

  record(failure: EmulationFailureInfo, context: string, owner = ownership.getStore()): void {
    const entry: Entry = { sequence: history.length + 1, journal: this.#id, context, failure: structuredClone(failure),
      ...(owner ? { owner } : {}) };
    history.push(entry);
    const state = owner && owners.get(owner.id);
    if (state?.closed) state.late.push(snapshot(entry));
  }
  get entries(): readonly RecordedEmulationFailure[] { return history.filter((entry) => entry.journal === this.#id).map(snapshot); }

  /** Capture ownership synchronously at API entry, never at eventual completion. */
  async run<T>(context: string, action: () => Promise<T>): Promise<T> {
    const work = { journal: this, owner: ownership.getStore(), context };
    outstanding.add(work);
    try { return await action(); }
    finally { outstanding.delete(work); }
  }
}

/** Called by the harness after all test hooks. Outstanding work fails immediately. */
export function verifyEmulationOwner(owner: EmulationOwner): void {
  const state = owners.get(owner.id)!;
  if (state.closed) throw new Error("Emulation owner verdict already closed");
  const pending = [...outstanding].filter((work) => work.owner?.id === owner.id);
  for (const work of pending) work.journal.record({ kind: "protocol", message: `outstanding work at test end: ${work.context}` }, work.context, owner);
  const entries = history.filter((entry) => entry.owner?.id === owner.id);
  const diagnostics = entries.flatMap((entry) => unexpected(entry).map((cause) => `${entry.context}: ${entry.failure.kind}: ${cause}`));
  coverage(owner);
  for (const claim of state.expectations) if (claim.cause === undefined) {
    diagnostics.push(`expected emulation failure did not occur: ${claim.name}`);
  }
  // Outstanding work cannot be waived by an expected-failure declaration.
  if (pending.length > 0 && diagnostics.length === 0) diagnostics.push("outstanding work at test end");
  if (owner.kind === "suite") {
    const descendant = (candidate: OwnerState): boolean => candidate.parent === owner.id || (candidate.parent !== undefined && descendant(owners.get(candidate.parent)!));
    for (const child of owners.values()) if (!child.failed && child.late.length > 0 && descendant(child)) {
      diagnostics.push(`late emulation failure after test end: ${child.owner.name}`);
    }
  }
  state.closed = true;
  state.failed = diagnostics.length > 0;
  if (diagnostics.length > 0) throw new Error(`Emulation failure guard (${owner.name}):\n${diagnostics.join("\n")}`);
}
