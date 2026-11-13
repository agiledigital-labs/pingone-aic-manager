// A client that forgets the superclass hook must get a bounded, clear error.
import type { RunnerTestFile } from "vitest";
import EmulationTestRunner from "../../../src/harness/vitest-runner.ts";
export default class BrokenRunner extends EmulationTestRunner {
  override onCollectStart(_file: RunnerTestFile): void {}
}
