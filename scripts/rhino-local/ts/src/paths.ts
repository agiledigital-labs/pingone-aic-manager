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

export const amEslintConfigPath = join(
  repoRoot,
  "src",
  "scripts",
  "templates",
  "am",
  "eslint.config.js"
);
