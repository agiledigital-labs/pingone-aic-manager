import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));

/** `scripts/rhino-local/ts/` */
export const packageRoot = join(here, "..");

/** pingone-aic-manager repo root. */
export const repoRoot = join(packageRoot, "..", "..", "..");

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

export const amRhinoEslintConfigPath = join(packageRoot, "eslint.am.config.js");

export const amEslintConfigPath = join(
  repoRoot,
  "src",
  "scripts",
  "templates",
  "am",
  "eslint.config.js"
);
