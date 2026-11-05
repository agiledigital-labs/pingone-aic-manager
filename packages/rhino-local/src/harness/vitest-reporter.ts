/** Main-process verdict: task inversion/retry/skip cannot change this exit status. */
import type { Reporter, Vitest, TestModule, RunnerTaskResultPack } from "vitest/node";
import { emulationAuditDiagnostics, type EmulationAudit } from "../emulation.ts";

import { EMULATION_REPORTER_CONTEXT, EMULATION_REPORTER_VERSION } from "./vitest-context.ts";
import { JVM_POOL_CONTEXT, JvmPool, poolEnabled } from "../pool.ts";
import { formatFaultTally, tallyFaults, writeFaultTally } from "./fault-tally.ts";

export default class EmulationReporter implements Reporter {
  #audits = new Map<string, EmulationAudit>();
  #ended = false;
  #printed = new Set<string>();

  #pool: Promise<JvmPool | undefined> | undefined;
  #watch = false;

  // The reporter is the package's one required main-process hook, so it owns
  // the JVM pool `useLease` borrows from (src/pool.ts). Vitest awaits onInit,
  // so the address is provided before any worker starts.
  async onInit(vitest: Vitest): Promise<void> {
    vitest.provide(EMULATION_REPORTER_CONTEXT, EMULATION_REPORTER_VERSION);
    this.#watch = vitest.config.watch;
    if (!poolEnabled()) return;
    this.#pool ??= JvmPool.start().catch((error: unknown) => {
      const message = error instanceof Error ? error.message : String(error);
      process.emitWarning(`rhino-local: JVM pool unavailable, each lease will start its own JVM: ${message}`);
      return undefined;
    });
    const pool = await this.#pool;
    if (pool !== undefined) vitest.provide(JVM_POOL_CONTEXT, pool.address);
  }

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
  async onTestRunEnd(modules: readonly TestModule[]): Promise<void> {
    for (const module of modules) this.#accept(module.meta());
    this.#ended = true;
    this.#enforce();
    this.#tally();
    // A watch run keeps its warm JVMs for the next rerun; idle ones never hold the process open.
    if (!this.#watch) await (await this.#pool)?.stop();
  }
  // Every worker's audit is here by now, so this covers all files, workers and
  // pooled JVMs. A tally that cannot be written is a warning, not a verdict:
  // the faults themselves already failed their cases.
  #tally(): void {
    const tally = tallyFaults(this.#audits.values());
    try {
      const path = writeFaultTally(tally);
      const summary = formatFaultTally(tally, path);
      if (summary !== "") process.stderr.write(summary);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      process.emitWarning(`rhino-local: could not write the harness-fault tally: ${message}`);
    }
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
