import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));

/** `packages/rhino-local/` */
export const packageRoot = join(here, "..");

/**
 * pingone-aic-manager repo root. **Repo-only**: generation and this repo's
 * tests read through it; nothing under `src/` may use it at run time,
 * because an installed package has no repo around it
 * (`test/paths.test.ts` enforces that).
 */
export const repoRoot = join(packageRoot, "..", "..");

/** The runner's Java sources (`packages/rhino-local/java/*.java`). */
export const javaSourceDir = join(packageRoot, "java");

/** The captured contexts JSON in this repo's docs. Repo-only; read by `generate`. */
export const sourceBindingsJsonPath = join(
  repoRoot,
  "docs",
  "api",
  "bindings",
  "scripted-decision-next.json"
);

export const generatedDir = join(packageRoot, "generated");

/** `generate`'s verbatim copy of {@link sourceBindingsJsonPath}, shipped with the package. */
export const bindingsJsonPath = join(generatedDir, "scripted-decision-next.json");

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

/** This repo's AM script lint. Repo-only: the lockstep test reads it. */
export const amEslintConfigPath = join(
  repoRoot,
  "src",
  "scripts",
  "templates",
  "am",
  "eslint.config.js"
);
