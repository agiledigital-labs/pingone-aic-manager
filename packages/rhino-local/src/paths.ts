import { existsSync, readFileSync } from "node:fs";
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
 * This package's own version, as `package.json` states it. Printed in the
 * fault tally and benchmark output so a report says which build produced it.
 */
export const packageVersion: string = (JSON.parse(readFileSync(join(packageRoot, "package.json"), "utf8")) as { version: string }).version;

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

/** The access-token-modification context's captured descriptor. Repo-only; read by `generate`. */
export const sourceAccessTokenBindingsJsonPath = join(repoRoot, "docs", "api", "bindings", "oauth2-atm-next.json");

/** `generate`'s verbatim copy of {@link sourceAccessTokenBindingsJsonPath}: the context's class allow-list. */
export const accessTokenBindingsJsonPath = join(generatedDir, "oauth2-atm-next.json");

/** The OIDC claims context's captured descriptor. Repo-only; read by `generate`. */
export const sourceOidcClaimsBindingsJsonPath = join(repoRoot, "docs", "api", "bindings", "oidc-claims-next.json");

/** `generate`'s verbatim copy of {@link sourceOidcClaimsBindingsJsonPath}: the context's class allow-list. */
export const oidcClaimsBindingsJsonPath = join(generatedDir, "oidc-claims-next.json");


/** The view helpers every non-decision context overlay builds on, appended before it. */
export const bindingsContextCommonPath = join(packageRoot, "src", "bindings", "rhino", "context-common.cjs");
/** The access-token-modification context's binding overlay, appended to the prelude for that context only. */
export const bindingsAccessTokenPath = join(packageRoot, "src", "bindings", "rhino", "context-oauth2-access-token.cjs");
/** The social IdP profile transformation context's captured descriptor. Repo-only; read by `generate`. */
export const sourceSocialIdpBindingsJsonPath = join(repoRoot, "docs", "api", "bindings", "social-idp-profile-transformation-next.json");

/** `generate`'s verbatim copy of {@link sourceSocialIdpBindingsJsonPath}: the context's class allow-list. */
export const socialIdpBindingsJsonPath = join(generatedDir, "social-idp-profile-transformation-next.json");

/** The social IdP profile transformation context's binding overlay, appended to the prelude for that context only. */
export const bindingsSocialIdpPath = join(packageRoot, "src", "bindings", "rhino", "context-social-idp.cjs");

/** The config provider node context's captured descriptor. Repo-only; read by `generate`. */
export const sourceConfigProviderBindingsJsonPath = join(repoRoot, "docs", "api", "bindings", "config-provider-node-next.json");

/** `generate`'s verbatim copy of {@link sourceConfigProviderBindingsJsonPath}: the context's class allow-list. */
export const configProviderBindingsJsonPath = join(generatedDir, "config-provider-node-next.json");

/** The config provider node context's binding overlay, appended to the prelude for that context only. */
export const bindingsConfigProviderPath = join(packageRoot, "src", "bindings", "rhino", "context-config-provider.cjs");

/** The validate scope context's captured next-gen descriptor. Repo-only; read by `generate`. */
export const sourceValidateScopeBindingsJsonPath = join(repoRoot, "docs", "api", "bindings", "oauth2-validate-scope-next.json");

/** `generate`'s verbatim copy of {@link sourceValidateScopeBindingsJsonPath}: the context's class allow-list. */
export const validateScopeBindingsJsonPath = join(generatedDir, "oauth2-validate-scope-next.json");

/** The validate scope context's binding overlay, appended to the prelude for that context only. */
export const bindingsValidateScopePath = join(packageRoot, "src", "bindings", "rhino", "context-oauth2-validate-scope.cjs");

/** The may-act context's captured next-gen descriptor. Repo-only; read by `generate`. */
export const sourceMayActBindingsJsonPath = join(repoRoot, "docs", "api", "bindings", "oauth2-may-act-next.json");

/** `generate`'s verbatim copy of {@link sourceMayActBindingsJsonPath}: the context's class allow-list. */
export const mayActBindingsJsonPath = join(generatedDir, "oauth2-may-act-next.json");

/** The may-act context's binding overlay, appended to the prelude for that context only. */
export const bindingsMayActPath = join(packageRoot, "src", "bindings", "rhino", "context-oauth2-may-act.cjs");

/** The library context's binding overlay, appended to the prelude for that context only. */
export const bindingsLibraryPath = join(packageRoot, "src", "bindings", "rhino", "context-library.cjs");

/** The OIDC claims context's binding overlay, appended to the prelude for that context only. */
export const bindingsOidcClaimsPath = join(packageRoot, "src", "bindings", "rhino", "context-oidc-claims.cjs");

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

/** The one CREST `_queryFilter` grammar, shared as text by Rhino and as CommonJS by Node. */
export const bindingsCrestFilterPath = join(
  packageRoot,
  "src",
  "bindings",
  "rhino",
  "crest-filter.cjs"
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
