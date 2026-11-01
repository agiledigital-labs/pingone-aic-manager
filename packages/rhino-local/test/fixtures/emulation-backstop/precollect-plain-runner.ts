import { fileURLToPath } from "node:url";
import { TestRunner } from "vitest";
export default class PlainPrecollectRunner extends TestRunner {
  async onBeforeCollect(_paths: string[]): Promise<void> {
    await this.importFile(fileURLToPath(new URL("./precollect-helper.ts", import.meta.url)), process.env.R7_IMPORT_SOURCE === "collect" ? "collect" : "setup");
  }
}
