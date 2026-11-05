/** Vitest worker transport. All final checks run outside test retry/skip/fails. */
import { TestRunner, type RunnerTaskResultPack, type RunnerTaskEventPack, type RunnerTestFile, type RunnerTestSuite, type RunnerTask } from "vitest";
import { emulationAudit, emulationAuditDiagnostics, finishEmulationFile, setEmulationFile, attachEmulationAuditTransport, type EmulationAudit } from "../emulation.ts";

// Keep file task handles across worker module graphs/batches, just as the core
// keeps journal history. A late owned entry can still fail its original file.
const fileTasksKey = Symbol.for("@agiledigital/pingone-aic-script-tester/vitest-file-tasks-v1");
const workerGlobal = process as typeof process & { [fileTasksKey]?: Map<string, RunnerTestFile> };
const fileTasks = workerGlobal[fileTasksKey] ??= new Map<string, RunnerTestFile>();

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
    const file = target ?? [...this.#files.values()].at(-1);
    if (!file) return;
    const audit = emulationAudit();
    const packs = new Map<string, RunnerTaskResultPack>();
    const include = (task: RunnerTask) => {
      (task.meta as { rhinoEmulationAudit?: EmulationAudit }).rhinoEmulationAudit = audit;
      packs.set(task.id, [task.id, task.result, task.meta]);
    };
    include(file);
    for (const completed of this.#files.values()) if (completed.result && completed.result.state !== "run") {
      const failures = audit.failures.filter((entry) => entry.file === completed.filepath || !entry.file);
      if (this.#failTask(completed, emulationAuditDiagnostics({ ...audit, failures, outstanding: [] }))) include(completed);
      const updateOwner = (task: RunnerTask): void => {
        if (task.type === "suite") { for (const child of task.tasks) updateOwner(child); return; }
        if (!task.result || task.result.state === "run" || task.result.state === "skip") return;
        const owned = failures.filter((entry) => entry.owner?.taskId === task.id);
        if (this.#failTask(task, emulationAuditDiagnostics({ ...audit, failures: owned, outstanding: [] }))) include(task);
      };
      updateOwner(completed);
    }
    if (!this.onTaskUpdate) return;
    const update = this.onTaskUpdate([...packs.values()], []).catch((error: unknown) => { this.#transportError = error; });
    this.#updates.add(update);
    void update.finally(() => { this.#updates.delete(update); });
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
    const audit = emulationAudit();
    const failures = audit.failures.filter((entry) => entry.owner?.taskId === task.id);
    this.#failTask(task, emulationAuditDiagnostics({ ...audit, failures, outstanding: [] }));
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
    const audit = emulationAudit();
    const failures = audit.failures.filter((entry) => entry.file === file.filepath || !entry.file);
    const messages = emulationAuditDiagnostics({ ...audit, failures, outstanding: [] });
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
      // instead, after the main process has taken it.
      if (this.#tracked && files.some((file) => !this.#finishedSuites.has(file.id))) {
        this.#backstop = backstop;
        return;
      }
      throw backstop;
    }
  }
}
