/** Vitest worker transport. All final checks run outside test retry/skip/fails. */
import { TestRunner, type RunnerTaskResultPack, type RunnerTaskEventPack, type RunnerTestFile, type RunnerTestSuite, type RunnerTask } from "vitest";
import { recordTiming, since, timingPath } from "../timing.ts";
import { emulationAudit, emulationAuditDelta, emulationAuditDiagnostics, emulationAuditWhere, finishEmulationFile, setEmulationFile, attachEmulationAuditTransport, type EmulationAudit } from "../emulation.ts";

// Keep file task handles across worker module graphs/batches, just as the core
// keeps journal history. A late owned entry can still fail its original file.
const fileTasksKey = Symbol.for("@agiledigital/pingone-aic-script-tester/vitest-file-tasks-v1");
const workerGlobal = process as typeof process & { [fileTasksKey]?: Map<string, RunnerTestFile> };
const fileTasks = workerGlobal[fileTasksKey] ??= new Map<string, RunnerTestFile>();

// Audit entries sent to the main process and not yet acknowledged, by
// sequence: each later send carries them again, so an entry is dropped only
// once a send that held it (or a newer version of it) has been answered.
// Process-wide for the same reason as the file tasks.
type AuditEntry = EmulationAudit["failures"][number];
const unackedKey = Symbol.for("@agiledigital/pingone-aic-script-tester/vitest-unacked-audit-v1");
const unackedGlobal = process as typeof process & { [unackedKey]?: Map<number, AuditEntry> };
const unacked = unackedGlobal[unackedKey] ??= new Map<number, AuditEntry>();

/**
 * Diagnostic fallback: send the whole audit at every boundary, as releases
 * before 0.4.0-emulator.2 did. The delta transport is compared against it in
 * test/harness/audit-delta.e2e.test.ts.
 */
export const AUDIT_SNAPSHOTS_ENV = "AIC_SCRIPT_TESTER_AUDIT_SNAPSHOTS";

function findTask(task: RunnerTask, id: string): RunnerTask | undefined {
  if (task.id === id) return task;
  if (task.type !== "suite") return undefined;
  for (const child of task.tasks) {
    const found = findTask(child, id);
    if (found) return found;
  }
  return undefined;
}

export default class EmulationTestRunner extends TestRunner {
  #files = fileTasks;
  #updates = new Set<Promise<unknown>>();
  #transportError: unknown;
  #collectionReady: WeakMap<RunnerTestFile, { promise: Promise<void>; resolve: () => void }> | undefined;
  #beforeCollectionFile: RunnerTestFile | undefined;

  /**
   * Vitest's own task-update sends: it wraps `onTaskUpdate` with its RPC before
   * the run starts, buffers task events behind a 100 ms throttle, and flushes
   * the buffer only after `onAfterRunFiles` returns. Wrapping it lets the
   * backstop below learn whether anything is still buffered (a file whose
   * `suite-finished` has not gone out) and throw only after the flush.
   */
  #finishedSuites = new Set<string>();
  #backstop: Error | undefined;
  #tracked = false;
  #trackTaskUpdates(): void {
    const send = this.onTaskUpdate;
    if (this.#tracked || !send) return;
    this.#tracked = true;
    this.onTaskUpdate = (packs, events) => {
      for (const [id, event] of events) if (event === "suite-finished") this.#finishedSuites.add(id);
      const update = send.call(this, packs, events);
      const backstop = this.#backstop;
      // The flush's send has reached the main process; now fail the run.
      return backstop ? update.then(() => { throw backstop; }) : update;
    };
  }

  #currentCollectionFile(): RunnerTestFile | undefined {
    // The public collector is created only when collection first starts.
    try { return TestRunner.getCurrentSuite().file; }
    catch { return undefined; }
  }

  #readinessFor(file: RunnerTestFile): { promise: Promise<void>; resolve: () => void } {
    if (!this.#collectionReady) throw new Error("Emulation runner requires super.onBeforeCollect before imports");
    let ready = this.#collectionReady.get(file);
    if (!ready) {
      let resolve!: () => void;
      const promise = new Promise<void>((signal) => { resolve = signal; });
      ready = { promise, resolve };
      this.#collectionReady.set(file, ready);
    }
    return ready;
  }

  // Vitest awaits the complete client hook before changing the collector's file.
  // Its later onCollectStart wrapper is NOT awaited before framework imports.
  onBeforeCollect(_paths: string[]): void {
    this.#trackTaskUpdates();
    this.#beforeCollectionFile = this.#currentCollectionFile();
    this.#collectionReady = new WeakMap();
    attachEmulationAuditTransport(this.#publish);
  }

  override async importFile(filepath: string, source: Parameters<TestRunner["importFile"]>[1]): Promise<unknown> {
    const file = this.#currentCollectionFile();
    // A client may import helpers while onBeforeCollect is still running. Wait
    // only once Vitest has installed a new file, independent of kind or path.
    if (!file || file === this.#beforeCollectionFile) return super.importFile(filepath, source);
    const ready = this.#readinessFor(file);
    const timeoutMs = this.config.hookTimeout || 10_000;
    let timeout: ReturnType<typeof setTimeout> | undefined;
    try {
      await Promise.race([ready.promise, new Promise<never>((_resolve, reject) => {
        timeout = setTimeout(() => reject(new Error(`Emulation runner collection readiness timed out after ${timeoutMs}ms; preserve super.onCollectStart and await asynchronous reporter hooks`)), timeoutMs);
      })]);
    } finally { clearTimeout(timeout); }
    return super.importFile(filepath, source);
  }

  // Vitest decorates this public runner hook with its worker-to-main RPC.
  onTaskUpdate?: (packs: RunnerTaskResultPack[], events: RunnerTaskEventPack[]) => Promise<void>;

  #publish = (target?: RunnerTestFile): void => {
    if (timingPath() === undefined) { this.#publishAudit(target); return; }
    const started = performance.now();
    const counts = this.#publishAudit(target);
    if (counts) recordTiming({ type: "publish", publishMs: since(started), ...counts });
  };

  /**
   * Sends what changed in the worker's audit to the main process's ledger
   * (src/emulation.ts EmulationAuditLedger, held by the reporter), plus every
   * entry not yet acknowledged; returns its size for the timing row. A
   * completed task that a new entry fails is marked and re-sent with it.
   */
  #publishAudit = (target?: RunnerTestFile): { failures: number; outstanding: number; packs: number } | undefined => {
    const file = target ?? [...this.#files.values()].at(-1);
    if (!file) return undefined;
    const snapshots = process.env[AUDIT_SNAPSHOTS_ENV] === "1";
    const delta = snapshots ? emulationAudit() : emulationAuditDelta();
    if (!snapshots) for (const entry of delta.failures) unacked.set(entry.sequence, entry);
    const sent = snapshots ? delta.failures : [...unacked.values()].sort((a, b) => a.sequence - b.sequence);
    const audit: EmulationAudit = { ...delta, failures: sent };
    const packs = new Map<string, RunnerTaskResultPack>();
    const include = (task: RunnerTask) => {
      (task.meta as { rhinoEmulationAudit?: EmulationAudit }).rhinoEmulationAudit = audit;
      packs.set(task.id, [task.id, task.result, task.meta]);
    };
    include(file);
    // Only an entry in this delta can change a completed task's verdict: the
    // rest were judged when they arrived.
    const affected = new Set<RunnerTask>();
    for (const entry of delta.failures) for (const completed of this.#files.values()) {
      if (!completed.result || completed.result.state === "run") continue;
      if (entry.file && entry.file !== completed.filepath) continue;
      affected.add(completed);
      const owner = entry.owner?.taskId === undefined ? undefined : findTask(completed, entry.owner.taskId);
      if (owner && owner.type !== "suite" && owner.result && owner.result.state !== "run" && owner.result.state !== "skip") affected.add(owner);
    }
    for (const task of affected) {
      const filepath = task.file.filepath;
      const inFile = (entry: { file?: string }) => entry.file === filepath || !entry.file;
      const owned = task.type === "suite" && "filepath" in task
        ? emulationAuditWhere(inFile)
        : emulationAuditWhere((entry) => inFile(entry) && entry.owner?.taskId === task.id);
      if (this.#failTask(task, emulationAuditDiagnostics({ ...owned, outstanding: [] }))) include(task);
    }
    const counts = { failures: sent.length, outstanding: audit.outstanding.length, packs: packs.size };
    if (!this.onTaskUpdate) return counts;
    const update = this.onTaskUpdate([...packs.values()], [])
      .then(() => {
        // Acknowledged: the reporter has merged this send. A newer version of an entry stays until its own send is.
        for (const entry of sent) if (unacked.get(entry.sequence) === entry) unacked.delete(entry.sequence);
      })
      .catch((error: unknown) => { this.#transportError = error; });
    this.#updates.add(update);
    void update.finally(() => { this.#updates.delete(update); });
    return counts;
  };

  override onCollectStart(file: RunnerTestFile): void {
    super.onCollectStart(file);
    this.#files.set(file.filepath, file);
    setEmulationFile(file.filepath);
    this.#publish();
    this.#readinessFor(file).resolve();
  }

  override onAfterRunTask(task: RunnerTask): void {
    super.onAfterRunTask(task);
    const owned = emulationAuditWhere((entry) => entry.owner?.taskId === task.id);
    this.#failTask(task, emulationAuditDiagnostics({ ...owned, outstanding: [] }));
  }

  #failTask(task: RunnerTask, messages: string[]): boolean {
    if (!messages.length) return false;
    let changed = task.result?.state !== "fail";
    task.result ??= { state: "fail" };
    task.result.state = "fail";
    const errors = task.result.errors ??= [];
    const message = messages.join("\n");
    if (!errors.some((error) => error.name === "EmulationFailureGuard" && error.message === message)) {
      errors.push({ name: "EmulationFailureGuard", message });
      changed = true;
    }
    return changed;
  }

  override async onAfterRunSuite(suite: RunnerTestSuite): Promise<void> {
    await super.onAfterRunSuite(suite);
    if ("filepath" in suite) this.#finishFile(suite as RunnerTestFile);
  }

  #finishFile(file: RunnerTestFile): void {
    finishEmulationFile(file.filepath);
    const failures = emulationAuditWhere((entry) => entry.file === file.filepath || !entry.file);
    const messages = emulationAuditDiagnostics({ ...failures, outstanding: [] });
    this.#failTask(file, messages);
    this.#publish(file);
  }

  override async onAfterRunFiles(files: RunnerTestFile[]): Promise<void> {
    for (const file of this.#files.values()) this.#finishFile(file);
    this.#publish();
    await Promise.all(this.#updates);
    super.onAfterRunFiles(files);
    if (this.#transportError) throw new Error("Emulation backstop audit transport failed", { cause: this.#transportError });
    // Supplement persistent failed task states; this throw alone is not a verdict.
    const diagnostics = emulationAuditDiagnostics(emulationAudit());
    if (diagnostics.length) {
      const backstop = new Error(`Emulation run backstop:\n${diagnostics.join("\n")}`);
      // Throwing here skips Vitest's final flush of its throttled task updates,
      // which can hold a file's last results indefinitely (its throttle does not
      // reschedule when the timer fires exactly at the interval). Those tests
      // then reached the main process as "pending", after any case's emulation
      // failure. When something is still buffered, fail the flush's send
      // instead, after the main process has taken it. If no send carries it,
      // the throw is lost — and so would a fallback's: a timer throwing here
      // after the run is not reported by Vitest (tried, 2026-11-05). The run
      // still fails, and the main reporter prints the message from its own
      // audit copy (R5 reporter-only, R9 dropped-flush in
      // emulation-backstop.e2e.test.ts).
      if (this.#tracked && files.some((file) => !this.#finishedSuites.has(file.id))) {
        this.#backstop = backstop;
        return;
      }
      throw backstop;
    }
  }
}
