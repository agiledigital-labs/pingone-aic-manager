import type { RunnerTestFile } from "vitest";
import PrecollectRunner from "./precollect-runner.ts";
import { canary } from "./canary-state.ts";
export default class CanaryRunner extends PrecollectRunner {
  override async onBeforeCollect(paths: string[]): Promise<void> {
    await super.onBeforeCollect(paths);
    canary.helperImports++;
  }
  override onCollectStart(file: RunnerTestFile): void {
    super.onCollectStart(file);
    canary.readyFile = file.filepath;
    canary.readyFiles.push(file.filepath);
  }
}
