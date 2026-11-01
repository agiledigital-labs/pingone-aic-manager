/** Vitest worker transport. All final checks run outside test retry/skip/fails. */
import { TestRunner, type RunnerTaskResultPack, type RunnerTaskEventPack, type RunnerTestFile, type RunnerTestSuite } from "vitest";
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

  override async onAfterRunSuite(suite: RunnerTestSuite): Promise<void> {
    await super.onAfterRunSuite(suite);
    if ("filepath" in suite) this.#finishFile(suite as RunnerTestFile);
  }

  #finishFile(file: RunnerTestFile): void {
    finishEmulationFile(file.filepath);
    const audit = emulationAudit();
    const unowned = audit.failures.filter((entry) => !entry.owner && (entry.file === file.filepath || !entry.file));
    const messages = emulationAuditDiagnostics({ ...audit, failures: unowned, outstanding: [] });
    if (messages.length && file.result) {
      file.result.state = "fail";
      (file.result.errors ??= []).push({ name: "EmulationFailureGuard", message: messages.join("\n") });
    }
    this.#publish();
  }

  override async onAfterRunFiles(files: RunnerTestFile[]): Promise<void> {
    for (const file of files) this.#finishFile(file);
    this.#publish();
    await Promise.all(this.#updates);
    super.onAfterRunFiles(files);
    if (this.#transportError) throw new Error("Emulation backstop audit transport failed", { cause: this.#transportError });
    // A worker final check also fails closed if the reporter was overridden by CLI.
    const diagnostics = emulationAuditDiagnostics(emulationAudit());
    if (diagnostics.length) throw new Error(`Emulation run backstop:\n${diagnostics.join("\n")}`);
  }
}
