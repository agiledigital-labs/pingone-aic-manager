/** Adversarial control: discard the worker's final thrown verdict. The main
 * process reporter must still fail the run from its independent audit copy. */
import type { RunnerTestFile } from "vitest";
import EmulationTestRunner from "../../../src/harness/vitest-runner.ts";
export default class ReporterOnlyRunner extends EmulationTestRunner {
  override async onAfterRunFiles(files: RunnerTestFile[]): Promise<void> {
    try { await super.onAfterRunFiles(files); }
    catch (error) { if (!(error instanceof Error) || !error.message.startsWith("Emulation run backstop:")) throw error; }
  }
}
