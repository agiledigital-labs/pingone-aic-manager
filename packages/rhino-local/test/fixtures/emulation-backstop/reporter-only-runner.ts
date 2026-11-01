/** Adversarial control: erase worker task verdicts and discard its final throw.
 * The main process reporter must still fail from its independent audit copy. */
import type { RunnerTask, RunnerTaskResultPack, RunnerTestFile } from "vitest";
import EmulationTestRunner from "../../../src/harness/vitest-runner.ts";
export default class ReporterOnlyRunner extends EmulationTestRunner {
  override async onAfterRunFiles(files: RunnerTestFile[]): Promise<void> {
    try { await super.onAfterRunFiles(files); }
    catch (error) { if (!(error instanceof Error) || !error.message.startsWith("Emulation run backstop:")) throw error; }
    const updates: RunnerTaskResultPack[] = [];
    const erase = (task: RunnerTask) => {
      if (task.result) { task.result.state = "pass"; delete task.result.errors; }
      updates.push([task.id, task.result, task.meta]);
      if (task.type === "suite") for (const child of task.tasks) erase(child);
    };
    for (const file of files) erase(file);
    await this.onTaskUpdate?.(updates, []);
  }
}
