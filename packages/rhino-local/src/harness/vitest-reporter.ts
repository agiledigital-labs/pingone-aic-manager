/** Main-process verdict: task inversion/retry/skip cannot change this exit status. */
import type { Reporter, TestModule, RunnerTaskResultPack } from "vitest/node";
import { emulationAuditDiagnostics, type EmulationAudit } from "../emulation.ts";

export default class EmulationReporter implements Reporter {
  #audits = new Map<string, EmulationAudit>();
  #ended = false;
  #printed = new Set<string>();

  onTestRunStart(): void { this.#audits.clear(); this.#printed.clear(); this.#ended = false; }
  #accept(meta: object): void {
    const audit = (meta as { rhinoEmulationAudit?: EmulationAudit }).rhinoEmulationAudit;
    if (audit && audit.revision >= (this.#audits.get(audit.id)?.revision ?? -1)) this.#audits.set(audit.id, audit);
  }
  // Vitest 5 still dispatches the task-update reporter hook, including after a
  // file's completion event. Never infer an audit verdict from task.result.
  onTaskUpdate(packs: RunnerTaskResultPack[]): void {
    for (const [, , meta] of packs) if (meta) this.#accept(meta);
    if (this.#ended) this.#enforce();
  }
  onTestRunEnd(modules: readonly TestModule[]): void {
    for (const module of modules) this.#accept(module.meta());
    this.#ended = true;
    this.#enforce();
  }
  #enforce(): void {
    for (const audit of this.#audits.values()) for (const message of emulationAuditDiagnostics(audit)) {
      process.exitCode = 1;
      if (!this.#printed.has(message)) {
        this.#printed.add(message);
        process.stderr.write(`Emulation run backstop: ${message}\n`);
      }
    }
  }
}
