import { readFileSync, statSync } from "node:fs";
import { normaliseIdentitySeeds } from "../case/identity.ts";
import type { Given } from "../case/types.ts";
import type { EnvProfile } from "../profile/types.ts";
import { managedWriteFaults } from "../profile/write-guard.ts";
import { bindingsCrestFilterPath, bindingsIdentityPolicyPath, bindingsRuntimePath, generatedJsPath } from "../paths.ts";

export interface MockPreambleOptions {
  /**
   * Optional override for direct runner callers. Suite cases carry libraries
   * in `given` and need no separate runner option.
   */
  libraries?: Record<string, string>;
  /**
   * Pulled environment schema. Only the set of declared object types, and the
   * per-type write faults `profile/write-guard.ts` decides (E15b), cross into
   * the sandbox — every schema rule is decided in the Node layer, so there is
   * one implementation of each rather than an AM-safe copy.
   */
  profile?: EnvProfile;
}

/**
 * Generated stubs + behaviour overlay + seed. Eval'd as the runner `preamble`
 * so author line numbers on `source` stay intact.
 *
 * Self-contained: the seed, library bodies included, is a literal in the
 * returned source. {@link runCase} sends the same thing in three parts
 * instead — see {@link casePreamble}.
 */
export function mockPreamble(
  given: Given = {},
  options: MockPreambleOptions = {}
): string {
  const { seed, overrides } = seedAndOverrides(given, options);
  return `${prelude()}__rhinoLocalSeed(${serializeSeed(seed)});\n${overrides}\n`;
}

/** {@link mockPreamble} as the runner job fields that carry it. */
export interface CasePreamble {
  /** Generated mocks, policy and runtime: identical for every case, so compiled once per JVM. */
  prelude: string;
  /** The seed call and binding overrides; continues the prelude's line numbers. */
  preamble: string;
  /** The seed as JSON text, read by `preamble` rather than compiled as a literal. */
  preambleData: string;
  /** Library bodies, kept out of the seed and compiled once per JVM per distinct body. */
  libraries?: Record<string, string>;
}

/**
 * The case preamble split so that the per-case part is small: run as
 * `prelude` then `preamble`, it does exactly what {@link mockPreamble}'s one
 * string does, at the same line numbers.
 */
export function casePreamble(
  given: Given = {},
  options: MockPreambleOptions = {}
): CasePreamble {
  const { seed, overrides } = seedAndOverrides(given, options);
  const { libraries, ...data } = seed;
  return {
    prelude: prelude(),
    preamble: `__rhinoLocalSeed(JSON.parse(__rhinoLocalPreambleData));\n${overrides}\n`,
    preambleData: serializeSeed(data),
    ...(libraries === undefined ? {} : { libraries: libraries as Record<string, string> }),
  };
}

function seedAndOverrides(
  given: Given,
  options: MockPreambleOptions
): { seed: Record<string, unknown>; overrides: string } {
  const seed: Record<string, unknown> = { ...normaliseIdentitySeeds(given) };
  if (options.libraries !== undefined) {
    seed.libraries = options.libraries;
  }
  if (options.profile !== undefined) {
    const known: Record<string, true> = {};
    for (const name of Object.keys(options.profile.objects)) {
      known[`managed/${name}`] = true;
    }
    seed.knownTypes = known;
    seed.profileTenant = options.profile.tenant;
    seed.profilePulledAt = options.profile.pulledAt;
    seed.managedWriteFaults = managedWriteFaults(options.profile);
  }
  const overrides = Object.entries(given.bindingOverrides ?? {})
    .map(([name, expression]) => `${name} = (${expression});`)
    .join("\n");
  return { seed, overrides };
}

let preludeMemo: { stamp: string; text: string } | undefined;

/**
 * The four package files, read once per process. Their size and mtime are
 * checked each call, so an edit during a watch run is still picked up.
 */
function prelude(): string {
  const files = [generatedJsPath, bindingsIdentityPolicyPath, bindingsCrestFilterPath, bindingsRuntimePath];
  const stamp = files
    .map((file) => {
      const stat = statSync(file);
      return `${stat.size}:${stat.mtimeMs}`;
    })
    .join("|");
  if (preludeMemo?.stamp !== stamp) {
    const [generated, policy, filter, runtime] = files.map((file) => readFileSync(file, "utf8"));
    preludeMemo = { stamp, text: `${generated}\n${policy}\n${filter}\n${runtime}\n` };
  }
  return preludeMemo.text;
}

/** Append a harvest call without shifting author line numbers. */
export function withHarvest(script: string): string {
  return `${script}\n;__rhinoLocalHarvest();\n`;
}

function serializeSeed(seed: unknown): string {
  return JSON.stringify(seed, (_key, value: unknown) => {
    if (value instanceof RegExp) {
      return { __regex: value.source, __flags: value.flags };
    }
    return value;
  });
}
