/** Adversarial control for the deferred run backstop: hide every
 * `suite-finished` from the runner's flush tracking, so the backstop is always
 * deferred to the final flush, then drop that flush. The worker's throw is then
 * lost; the run must still fail from the main reporter's audit copy. */
import type { RunnerTaskEventPack, RunnerTaskResultPack, RunnerTestFile } from "vitest";
import EmulationTestRunner from "../../../src/harness/vitest-runner.ts";
export default class FlushDroppedRunner extends EmulationTestRunner {
  override onBeforeCollect(paths: string[]): void {
    super.onBeforeCollect(paths);
    const tracked = this.onTaskUpdate;
    if (!tracked || (tracked as { hidden?: true }).hidden) return;
    const hide = (packs: RunnerTaskResultPack[], events: RunnerTaskEventPack[]) =>
      tracked.call(this, packs, events.filter(([, event]) => event !== "suite-finished"));
    this.onTaskUpdate = Object.assign(hide, { hidden: true as const });
  }
  override async onAfterRunFiles(files: RunnerTestFile[]): Promise<void> {
    await super.onAfterRunFiles(files);
    this.onTaskUpdate = async () => {};
  }
}
