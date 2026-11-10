import { readFileSync, statSync } from "node:fs";
import { normaliseIdentitySeeds } from "../case/identity.ts";
import type { Given } from "../case/types.ts";
import { checkProfileObjects } from "../profile/normalise.ts";
import type { EnvProfile } from "../profile/types.ts";
import { managedInvalidDefault, managedPolicy, managedWriteFaults } from "../profile/write-guard.ts";
import {
  bindingsAccessTokenPath,
  bindingsCrestFilterPath,
  bindingsIdentityPolicyPath,
  bindingsRuntimePath,
  generatedJsPath,
} from "../paths.ts";
import type { ScriptContext } from "../case/types.ts";
import { CONTEXT_ONLY_GIVEN } from "../case/context.ts";

export interface MockPreambleOptions {
  /**
   * Optional override for direct runner callers. Suite cases carry libraries
   * in `given` and need no separate runner option.
   */
  libraries?: Record<string, string>;
  /**
   * Pulled environment schema. Only the set of declared object types, and the
   * tables `profile/write-guard.ts` decides (per-type write faults and
   * implicit policy, and the one fault an invalid default anywhere raises on
   * every managed call), cross into
   * the sandbox — every schema rule is decided in the Node layer, so there is
   * one implementation of each rather than an AM-safe copy.
   */
  profile?: EnvProfile;
  /**
   * The script context (`case.context`). Absent or the decision node: the
   * preamble is exactly what it was before contexts existed.
   */
  context?: ScriptContext;
}

/** Each non-decision context's overlay, seed call and harvest call. */
const CONTEXT_RUNTIME: { readonly [K in ScriptContext]?: { overlay: string; seed: string; harvest: string } } = {
  OAUTH2_ACCESS_TOKEN_MODIFICATION: {
    overlay: bindingsAccessTokenPath,
    seed: "__rhinoLocalSeedAccessToken",
    harvest: "__rhinoLocalHarvestAccessToken",
  },
};

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
  return `${prelude(options.context)}${seedCall(options.context, seedLiteral(serializeSeed(seed)))}\n${overrides}\n`;
}

/**
 * The seed as source text. JSON is a JavaScript literal except for one key: in an object literal
 * `"__proto__": value` sets the prototype, where JSON.parse defines an own key (astra-r1 P2-2), so a
 * profile property of that name would vanish from the policy tables. A seed holding that key is
 * parsed instead; every other seed stays a literal, as before.
 */
function seedLiteral(data: string): string {
  if (!data.includes('"__proto__"')) return data;
  return `JSON.parse(${JSON.stringify(data).replace(/\u2028/g, "\\u2028").replace(/\u2029/g, "\\u2029")})`;
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
    prelude: prelude(options.context),
    preamble: `${seedCall(options.context, "JSON.parse(__rhinoLocalPreambleData)")}\n${overrides}\n`,
    preambleData: serializeSeed(data),
    ...(libraries === undefined ? {} : { libraries: libraries as Record<string, string> }),
  };
}

/** One statement, so the preamble's line count does not depend on the context. */
function seedCall(context: ScriptContext | undefined, data: string): string {
  const runtime = context === undefined ? undefined : CONTEXT_RUNTIME[context];
  if (runtime === undefined) {
    return `__rhinoLocalSeed(${data});`;
  }
  return `(function (g) { __rhinoLocalSeed(g); ${runtime.seed}(g); })(${data});`;
}

/** The harvest call a context's script ends with. */
export function harvestCall(context: ScriptContext | undefined): string {
  const runtime = context === undefined ? undefined : CONTEXT_RUNTIME[context];
  return runtime === undefined ? "__rhinoLocalHarvest" : runtime.harvest;
}

function seedAndOverrides(
  given: Given,
  options: MockPreambleOptions
): { seed: Record<string, unknown>; overrides: string } {
  const seed: Record<string, unknown> = { ...normaliseIdentitySeeds(given) };
  if (options.context === undefined || CONTEXT_RUNTIME[options.context] === undefined) {
    // A decision-node seed never carries a context's fixtures (validation
    // refuses them there; a direct caller's are dropped, not half-applied).
    for (const key of CONTEXT_ONLY_GIVEN) delete seed[key];
  } else if (options.context === "OAUTH2_ACCESS_TOKEN_MODIFICATION") {
    // The token's default auth time is "now", resolved here rather than in the
    // JVM so the seed is data: AIC_SCRIPT_TESTER_JVM=both sends both lanes the
    // same request, and a clock read in each would make them disagree.
    const token = (seed.accessToken ?? {}) as Record<string, unknown>;
    if (token.authTimeSeconds === undefined) {
      seed.accessToken = { ...token, authTimeSeconds: Math.floor(Date.now() / 1000) };
    }
  }
  if (options.libraries !== undefined) {
    seed.libraries = options.libraries;
  }
  if (options.profile !== undefined) {
    // A runner caller can hand over a profile object that never went through parseProfile.
    checkProfileObjects(options.profile, "given to the case");
    const known: Record<string, true> = {};
    for (const name of Object.keys(options.profile.objects)) {
      known[`managed/${name}`] = true;
    }
    seed.knownTypes = known;
    seed.profileTenant = options.profile.tenant;
    seed.profilePulledAt = options.profile.pulledAt;
    seed.managedWriteFaults = managedWriteFaults(options.profile);
    seed.managedPolicy = managedPolicy(options.profile);
    seed.managedInvalidDefault = managedInvalidDefault(options.profile) ?? null;
  }
  const overrides = Object.entries(given.bindingOverrides ?? {})
    .map(([name, expression]) => `${name} = (${expression});`)
    .join("\n");
  return { seed, overrides };
}

const preludeMemo = new Map<string, { stamp: string; text: string }>();

/**
 * The four package files, plus the context's overlay when the case names a
 * context, read once per process. Their size and mtime are checked each call,
 * so an edit during a watch run is still picked up. The decision node's text
 * is unchanged by contexts existing, so its runner cache key is too.
 */
function prelude(context?: ScriptContext): string {
  const overlay = context === undefined ? undefined : CONTEXT_RUNTIME[context]?.overlay;
  const files = [generatedJsPath, bindingsIdentityPolicyPath, bindingsCrestFilterPath, bindingsRuntimePath];
  if (overlay !== undefined) files.push(overlay);
  const stamp = files
    .map((file) => {
      const stat = statSync(file);
      return `${stat.size}:${stat.mtimeMs}`;
    })
    .join("|");
  const key = overlay ?? "";
  const memo = preludeMemo.get(key);
  if (memo?.stamp === stamp) return memo.text;
  const text = files.map((file) => `${readFileSync(file, "utf8")}\n`).join("");
  preludeMemo.set(key, { stamp, text });
  return text;
}

/** Append a harvest call without shifting author line numbers. */
export function withHarvest(script: string, context?: ScriptContext): string {
  return `${script}\n;${harvestCall(context)}();\n`;
}

function serializeSeed(seed: unknown): string {
  return JSON.stringify(seed, (_key, value: unknown) => {
    if (value instanceof RegExp) {
      return { __regex: value.source, __flags: value.flags };
    }
    return value;
  });
}
