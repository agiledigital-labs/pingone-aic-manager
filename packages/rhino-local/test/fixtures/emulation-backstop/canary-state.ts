import { threadId } from "node:worker_threads";
import { TestRunner } from "vitest";
interface FileImports { setup?: string; test?: string }
interface CanaryState {
  readyFile?: string;
  readyFiles: string[];
  helperImports: number;
  imports: Map<string, FileImports>;
}
const key = Symbol.for("rhino-local/readiness-upgrade-canary");
const shared = process as typeof process & { [key]?: CanaryState };
export const canary = shared[key] ??= { readyFiles: [], helperImports: 0, imports: new Map() };

export function recordFrameworkImport(stage: "setup" | "test"): string {
  const file = TestRunner.getCurrentSuite().file.filepath;
  if (canary.readyFile !== file) {
    throw new Error(`Readiness upgrade canary: ${stage} import did not await its own file readiness: ${file}; ready file: ${canary.readyFile ?? "none"}`);
  }
  const entry = canary.imports.get(file) ?? {};
  entry[stage] = canary.readyFile;
  canary.imports.set(file, entry);
  return file;
}
export function evidence(file: string) {
  return { file, setupReadyFile: canary.imports.get(file)?.setup, testReadyFile: canary.imports.get(file)?.test,
    readyFiles: [...canary.readyFiles], helperImports: canary.helperImports,
    helper: (globalThis as typeof globalThis & { precollectHelper?: string }).precollectHelper,
    worker: `${process.pid}:${threadId}` };
}
