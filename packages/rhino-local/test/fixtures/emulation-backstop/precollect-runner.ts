import { fileURLToPath } from "node:url";
import EmulationTestRunner from "../../../src/harness/vitest-runner.ts";
export default class PrecollectRunner extends EmulationTestRunner {
  override async onBeforeCollect(paths: string[]): Promise<void> {
    super.onBeforeCollect(paths);
    await this.importFile(fileURLToPath(new URL("./precollect-helper.ts", import.meta.url)), process.env.R7_IMPORT_SOURCE === "collect" ? "collect" : "setup");
  }
}
