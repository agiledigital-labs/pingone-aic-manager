/**
 * The directory holding this package's `package.json`: `packages/rhino-local/`
 * in the repo, `node_modules/<name>/` installed. Found by walking up, because
 * the compiled copy of this file sits one level deeper (`dist/src/`) than the
 * source does.
 */
export declare const packageRoot: string;
/**
 * pingone-aic-manager repo root. **Repo-only**: generation and this repo's
 * tests read through it; nothing under `src/` may use it at run time,
 * because an installed package has no repo around it
 * (`test/paths.test.ts` enforces that).
 */
export declare const repoRoot: string;
/** The runner's Java sources (`packages/rhino-local/java/*.java`). */
export declare const javaSourceDir: string;
/**
 * Runner classes compiled at build time and shipped, so a consumer needs a
 * `java` but no `javac`. Used only while their manifest matches
 * {@link javaSourceDir}; absent in a checkout until `npm run build`.
 */
export declare const prebuiltClassesDir: string;
/** The captured contexts JSON in this repo's docs. Repo-only; read by `generate`. */
export declare const sourceBindingsJsonPath: string;
export declare const generatedDir: string;
/** `generate`'s verbatim copy of {@link sourceBindingsJsonPath}, shipped with the package. */
export declare const bindingsJsonPath: string;
export declare const generatedJsPath: string;
export declare const generatedDtsPath: string;
/** Handwritten AM-safe overlay that implements a subset of the generated stubs. */
export declare const bindingsRuntimePath: string;
/** Scripted-decision case files (author scripts + defineCase wrappers). */
export declare const casesDir: string;
export declare const amRhinoEslintConfigPath: string;
/** This repo's AM script lint. Repo-only: the lockstep test reads it. */
export declare const amEslintConfigPath: string;
