/** Vitest worker transport. All final checks run outside test retry/skip/fails. */
import { TestRunner, type RunnerTaskResultPack, type RunnerTaskEventPack, type RunnerTestFile, type RunnerTestSuite, type RunnerTask } from "vitest";
import { emulationAudit, emulationAuditDiagnostics, finishEmulationFile, setEmulationFile, attachEmulationAuditTransport, type EmulationAudit } from "../emulation.ts";

export default class EmulationTestRunner extends TestRunner {
  #files = new Map<string, RunnerTestFile>();
  #updates = new Set<Promise<unknown>>();
  #transportError: unknown;
  #subscribed = false;

  // Vitest decorates this public runner hook with its worker-to-main RPC.
  onTaskUpdate?: (packs: RunnerTaskResultPack[], events: RunnerTaskEventPack[]) => Promise<void>;

  #publish = (): void => {
    const file = [...this.#files.values()].at(-1);
    if (!file) return;
    (file.meta as { rhinoEmulationAudit?: EmulationAudit }).rhinoEmulationAudit = emulationAudit();
    if (!this.onTaskUpdate) return;
    const update = this.onTaskUpdate([[file.id, file.result, file.meta]], []).catch((error: unknown) => { this.#transportError = error; });
    this.#updates.add(update);
    void update.finally(() => { this.#updates.delete(update); });
  };

  override onCollectStart(file: RunnerTestFile): void {
    super.onCollectStart(file);
    this.#files.set(file.filepath, file);
    setEmulationFile(file.filepath);
    if (!this.#subscribed) { attachEmulationAuditTransport(this.#publish); this.#subscribed = true; }
    this.#publish();
  }

  override onAfterRunTask(task: RunnerTask): void {
    super.onAfterRunTask(task);
    const audit = emulationAudit();
    const failures = audit.failures.filter((entry) => entry.owner?.taskId === task.id);
    this.#failTask(task, emulationAuditDiagnostics({ ...audit, failures, outstanding: [] }));
  }

  #failTask(task: RunnerTask, messages: string[]): void {
    if (!messages.length) return;
    task.result ??= { state: "fail" };
    task.result.state = "fail";
    const errors = task.result.errors ??= [];
    const message = messages.join("\n");
    if (!errors.some((error) => error.name === "EmulationFailureGuard" && error.message === message)) {
      errors.push({ name: "EmulationFailureGuard", message });
    }
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
    this.#publish();
  }

  override async onAfterRunFiles(files: RunnerTestFile[]): Promise<void> {
    for (const file of files) this.#finishFile(file);
    this.#publish();
    await Promise.all(this.#updates);
    super.onAfterRunFiles(files);
    if (this.#transportError) throw new Error("Emulation backstop audit transport failed", { cause: this.#transportError });
    // Supplement persistent failed task states; this throw alone is not a verdict.
    const diagnostics = emulationAuditDiagnostics(emulationAudit());
    if (diagnostics.length) throw new Error(`Emulation run backstop:\n${diagnostics.join("\n")}`);
  }
}
