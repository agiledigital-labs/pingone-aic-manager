import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));

/** `scripts/rhino-local/ts/` */
export const packageRoot = join(here, "..");

/** pingone-aic-manager repo root. */
export const repoRoot = join(packageRoot, "..", "..", "..");

/** Long-lived JVM runner launcher (`scripts/rhino-local/run-runner.sh`). */
export const runRunnerScript = join(packageRoot, "..", "run-runner.sh");

export const bindingsJsonPath = join(
  repoRoot,
  "docs",
  "api",
  "bindings",
  "scripted-decision-next.json"
);

export const generatedDir = join(packageRoot, "generated");

export const generatedJsPath = join(
  generatedDir,
  "scripted-decision-mocks.cjs"
);

export const generatedDtsPath = join(
  generatedDir,
  "scripted-decision-mocks.d.ts"
);

/** Handwritten AM-safe overlay that implements a subset of the generated stubs. */
export const bindingsRuntimePath = join(
  packageRoot,
  "src",
  "bindings",
  "rhino",
  "runtime.cjs"
);

/** Scripted-decision case files (author scripts + defineCase wrappers). */
export const casesDir = join(packageRoot, "cases");

export const amRhinoEslintConfigPath = join(packageRoot, "eslint.am.config.js");

export const amEslintConfigPath = join(
  repoRoot,
  "src",
  "scripts",
  "templates",
  "am",
  "eslint.config.js"
);

/** Per-checkout dump of failed AIC-lane tests. Gitignored; not `/tmp`. */
export const failuresDir = join(packageRoot, "failures");

export const failuresPath = join(failuresDir, "failures.jsonl");

export const latestLogsPath = join(failuresDir, "latest-logs.json");
