import { readFileSync } from "node:fs";
import type { Given } from "../case/types.ts";
import type { EnvProfile } from "../profile/types.ts";
import { bindingsRuntimePath, generatedJsPath } from "../paths.ts";

export interface MockPreambleOptions {
  /**
   * Optional override for direct runner callers. Suite cases carry libraries
   * in `given` and need no separate runner option.
   */
  libraries?: Record<string, string>;
  /**
   * Pulled environment schema. Only the set of declared object types crosses
   * into the sandbox — every schema rule is enforced in the Node layer, so
   * there is one implementation of each rather than an AM-safe copy.
   */
  profile?: EnvProfile;
}

/**
 * Generated stubs + behaviour overlay + seed. Eval'd as the runner `preamble`
 * so author line numbers on `source` stay intact.
 */
export function mockPreamble(
  given: Given = {},
  options: MockPreambleOptions = {}
): string {
  const generated = readFileSync(generatedJsPath, "utf8");
  const runtime = readFileSync(bindingsRuntimePath, "utf8");
  const seed: Record<string, unknown> = { ...given };
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
  }
  const overrides = Object.entries(given.bindingOverrides ?? {})
    .map(([name, expression]) => `${name} = (${expression});`)
    .join("\n");
  return `${generated}\n${runtime}\n__rhinoLocalSeed(${serializeSeed(seed)});\n${overrides}\n`;
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
