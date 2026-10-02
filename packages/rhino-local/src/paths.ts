import { existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));

/**
 * The directory holding this package's `package.json`: `packages/rhino-local/`
 * in the repo, `node_modules/<name>/` installed. Found by walking up, because
 * the compiled copy of this file sits one level deeper (`dist/src/`) than the
 * source does.
 */
export const packageRoot = findPackageRoot(here);

function findPackageRoot(start: string): string {
  for (let dir = start; ; dir = dirname(dir)) {
    if (existsSync(join(dir, "package.json"))) {
      return dir;
    }
    if (dirname(dir) === dir) {
      throw new Error(`rhino-local: no package.json above ${start}`);
    }
  }
}

/**
 * pingone-aic-manager repo root. **Repo-only**: generation and this repo's
 * tests read through it; nothing under `src/` may use it at run time,
 * because an installed package has no repo around it
 * (`test/paths.test.ts` enforces that).
 */
export const repoRoot = join(packageRoot, "..", "..");

/** The runner's Java sources (`packages/rhino-local/java/*.java`). */
export const javaSourceDir = join(packageRoot, "java");

/**
 * Runner classes compiled at build time and shipped, so a consumer needs a
 * `java` but no `javac`. Used only while their manifest matches
 * {@link javaSourceDir}; absent in a checkout until `npm run build`.
 */
export const prebuiltClassesDir = join(packageRoot, "dist", "classes");

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

/** Canonical AM-safe identity policy, shared as text by Rhino and as CommonJS by Node. */
export const bindingsIdentityPolicyPath = join(
  packageRoot,
  "src",
  "bindings",
  "rhino",
  "identity-policy.cjs"
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
