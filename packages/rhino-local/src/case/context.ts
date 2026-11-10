/**
 * Script contexts: what each one accepts in a case. The runtime half (which
 * bindings exist and how they behave) is the context's overlay under
 * `src/bindings/rhino/`; this file is the validation half, so a case that
 * names a key its context cannot see fails before it runs.
 * packages/rhino-local/docs/script-contexts.md.
 */
import { DEFAULT_SCRIPT_CONTEXT, SCRIPT_CONTEXTS } from "./types.ts";
import type {
  AccessTokenExpect,
  AccessTokenSeed,
  AllowUndeclaredChannel,
  ClientSeed,
  Engine,
  ExpectedValue,
  ExpectKey,
  GivenKey,
  IdentitySeed,
  ScriptContext,
  TokenRequestSeed,
} from "./types.ts";
import { assertDenseArray, defineOwn, isPlainObject, parseJsonObject, unknownKeyError } from "./util.ts";

export interface ContextDeclaration {
  id: ScriptContext;
  /** Engines AM has the context on. */
  engines: readonly Engine[];
  /** `given` keys the context accepts. */
  givenKeys: readonly GivenKey[];
  /** `expect` keys the context accepts. */
  expectKeys: readonly ExpectKey[];
  /** `allowUndeclared` flags that mean something here. */
  allowUndeclared: readonly AllowUndeclaredChannel[];
  /** Whether the script decides an outcome (only the decision node does). */
  hasOutcome: boolean;
}

/** The keys every non-decision context shares: environment, not state. */
const SHARED_GIVEN: readonly GivenKey[] = [
  "engine",
  "realm",
  "scriptName",
  "esv",
  "esvUndeclared",
  "secrets",
  "libraries",
  "managed",
  "http",
  "openidmActions",
  "openidmFailures",
  "bindingOverrides",
];

/** `given` keys only a non-decision context may carry. */
export const CONTEXT_ONLY_GIVEN: readonly GivenKey[] = ["accessToken", "identity", "client", "scopes", "request"];
export const CONTEXT_ONLY_EXPECT: readonly ExpectKey[] = ["accessToken"];
export const CONTEXT_ONLY_ALLOW: readonly AllowUndeclaredChannel[] = ["accessToken"];

export const CONTEXTS: { readonly [K in ScriptContext]: ContextDeclaration } = {
  SCRIPTED_DECISION_NODE: {
    id: "SCRIPTED_DECISION_NODE",
    engines: ["next-gen", "legacy"],
    givenKeys: [],
    expectKeys: [],
    allowUndeclared: [],
    hasOutcome: true,
  },
  OAUTH2_ACCESS_TOKEN_MODIFICATION: {
    id: "OAUTH2_ACCESS_TOKEN_MODIFICATION",
    engines: ["next-gen", "legacy"],
    givenKeys: [...SHARED_GIVEN, "accessToken", "identity", "client", "scopes", "request"],
    expectKeys: ["outcome", "accessToken", "openidm", "http", "logs", "allowUndeclared"],
    allowUndeclared: ["accessToken", "openidmWrites", "openidmReads", "http", "logs"],
    hasOutcome: false,
  },
};

export function parseContext(raw: unknown, path: string): ScriptContext {
  if (raw === undefined) {
    return DEFAULT_SCRIPT_CONTEXT;
  }
  if (typeof raw !== "string" || !(SCRIPT_CONTEXTS as readonly string[]).includes(raw)) {
    const hint =
      typeof raw === "string" && raw.endsWith("_NEXT_GEN")
        ? `; drop the _NEXT_GEN suffix and set given.engine ("next-gen" is the default)`
        : "";
    throw new Error(
      `rhino-local: ${path} must be one of ${SCRIPT_CONTEXTS.map((c) => JSON.stringify(c)).join(", ")}${hint}`
    );
  }
  return raw as ScriptContext;
}

/**
 * Refuse a key the context cannot see. For the decision node only the
 * context-only keys are refused, so every existing case validates as before.
 */
export function checkContextKeys(
  context: ScriptContext,
  section: "given" | "expect" | "allowUndeclared",
  present: readonly string[],
  path: string
): void {
  const decl = CONTEXTS[context];
  if (decl.hasOutcome) {
    const refused: readonly string[] =
      section === "given" ? CONTEXT_ONLY_GIVEN : section === "expect" ? CONTEXT_ONLY_EXPECT : CONTEXT_ONLY_ALLOW;
    for (const key of present) {
      if (refused.includes(key)) {
        throw new Error(
          `rhino-local: ${path}.${key} belongs to a script context, not the decision node; set case.context (for example "OAUTH2_ACCESS_TOKEN_MODIFICATION")`
        );
      }
    }
    return;
  }
  const allowed: readonly string[] =
    section === "given" ? decl.givenKeys : section === "expect" ? decl.expectKeys : decl.allowUndeclared;
  for (const key of present) {
    if (!allowed.includes(key)) {
      throw new Error(
        `rhino-local: ${path}.${key} is not accepted in context ${context}; it accepts ${allowed.join(", ")}`
      );
    }
  }
}

const SEED_KEYS = [
  "clientId",
  "resourceOwnerId",
  "grantType",
  "realm",
  "scope",
  "audience",
  "tokenName",
  "tokenType",
  "authTimeSeconds",
  "expiryTime",
  "fields",
  "nonce",
  "authLevel",
  "claims",
  "tokenId",
  "auditTrackingId",
  "authGrantId",
] as const;

export function parseAccessTokenSeed(raw: unknown, path: string): AccessTokenSeed {
  const obj = plain(raw, path, SEED_KEYS);
  const out: AccessTokenSeed = {};
  for (const key of ["clientId", "resourceOwnerId", "grantType", "realm", "tokenName", "tokenType", "tokenId", "auditTrackingId", "authGrantId"] as const) {
    if (obj[key] !== undefined) out[key] = str(obj[key], `${path}.${key}`);
  }
  if (obj.scope !== undefined) out.scope = scopeSet(obj.scope, `${path}.scope`);
  if (obj.audience !== undefined) out.audience = strings(obj.audience, `${path}.audience`);
  if (obj.authTimeSeconds !== undefined) out.authTimeSeconds = integer(obj.authTimeSeconds, `${path}.authTimeSeconds`);
  if (obj.expiryTime !== undefined) out.expiryTime = integer(obj.expiryTime, `${path}.expiryTime`);
  if (obj.fields !== undefined) out.fields = parseJsonObject(obj.fields, `${path}.fields`);
  if (obj.nonce !== undefined) out.nonce = obj.nonce === null ? null : str(obj.nonce, `${path}.nonce`);
  if (obj.authLevel !== undefined) out.authLevel = obj.authLevel === null ? null : integer(obj.authLevel, `${path}.authLevel`);
  if (obj.claims !== undefined) out.claims = obj.claims === null ? null : str(obj.claims, `${path}.claims`);
  return out;
}

export function parseIdentitySeed(raw: unknown, path: string): IdentitySeed {
  const obj = plain(raw, path, ["name", "universalId", "exists", "attributes"]);
  const out: IdentitySeed = {};
  if (obj.name !== undefined) out.name = str(obj.name, `${path}.name`);
  if (obj.universalId !== undefined) out.universalId = str(obj.universalId, `${path}.universalId`);
  if (obj.exists !== undefined) {
    if (typeof obj.exists !== "boolean") throw new Error(`rhino-local: ${path}.exists must be a boolean`);
    out.exists = obj.exists;
  }
  if (obj.attributes !== undefined) out.attributes = stringArrayMap(obj.attributes, `${path}.attributes`);
  return out;
}

export function parseClientSeed(raw: unknown, path: string): ClientSeed {
  const obj = plain(raw, path, ["clientId", "allowedGrantTypes", "allowedScopes", "allowedResponseTypes", "customProperties", "redirectUris"]);
  const out: ClientSeed = {};
  if (obj.clientId !== undefined) out.clientId = str(obj.clientId, `${path}.clientId`);
  for (const key of ["allowedGrantTypes", "allowedScopes", "allowedResponseTypes", "redirectUris"] as const) {
    if (obj[key] !== undefined) out[key] = strings(obj[key], `${path}.${key}`);
  }
  if (obj.customProperties !== undefined) {
    const custom = plain(obj.customProperties, `${path}.customProperties`);
    const map: Record<string, string> = {};
    for (const [k, v] of Object.entries(custom)) defineOwn(map, k, str(v, `${path}.customProperties.${k}`));
    out.customProperties = map;
  }
  return out;
}

export function parseTokenRequestSeed(raw: unknown, path: string): TokenRequestSeed {
  const obj = plain(raw, path, ["params", "headers", "uri"]);
  const out: TokenRequestSeed = {};
  if (obj.params !== undefined) out.params = stringArrayMap(obj.params, `${path}.params`);
  if (obj.headers !== undefined) out.headers = stringArrayMap(obj.headers, `${path}.headers`);
  if (obj.uri !== undefined) out.uri = str(obj.uri, `${path}.uri`);
  return out;
}

const EXPECT_TOKEN_KEYS = [
  "fields",
  "responseFields",
  "scope",
  "expiryTime",
  "nonce",
  "authLevel",
  "claims",
  "act",
  "mayAct",
  "permissions",
  "issued",
] as const;

/** Keys of {@link AccessTokenExpect} whose value is an `ExpectedValue`. */
export const TOKEN_VALUE_KEYS = ["expiryTime", "nonce", "authLevel", "claims", "act", "mayAct", "permissions"] as const;

export function parseAccessTokenExpect(
  raw: unknown,
  path: string,
  parsers: {
    stateDiff: (raw: unknown, path: string) => NonNullable<AccessTokenExpect["fields"]>;
    expectedValue: (raw: unknown, path: string) => ExpectedValue;
  }
): AccessTokenExpect {
  const obj = plain(raw, path, EXPECT_TOKEN_KEYS);
  const out: AccessTokenExpect = {};
  if (obj.fields !== undefined) out.fields = parsers.stateDiff(obj.fields, `${path}.fields`);
  if (obj.responseFields !== undefined) out.responseFields = parsers.stateDiff(obj.responseFields, `${path}.responseFields`);
  if (obj.scope !== undefined) out.scope = scopeSet(obj.scope, `${path}.scope`);
  for (const key of TOKEN_VALUE_KEYS) {
    if (key in obj) out[key] = parsers.expectedValue(obj[key], `${path}.${key}`);
  }
  if (obj.issued !== undefined) {
    if (typeof obj.issued !== "boolean") throw new Error(`rhino-local: ${path}.issued must be a boolean`);
    out.issued = obj.issued;
  }
  return out;
}

function plain(raw: unknown, path: string, keys?: readonly string[]): Record<string, unknown> {
  if (!isPlainObject(raw)) {
    throw new Error(`rhino-local: ${path} is not an object`);
  }
  if (keys !== undefined) {
    for (const key of Object.keys(raw)) {
      if (!keys.includes(key)) throw unknownKeyError(path, key, keys);
    }
  }
  return raw;
}

function str(raw: unknown, path: string): string {
  if (typeof raw !== "string") throw new Error(`rhino-local: ${path} must be a string`);
  return raw;
}

function integer(raw: unknown, path: string): number {
  if (typeof raw !== "number" || !Number.isSafeInteger(raw)) throw new Error(`rhino-local: ${path} must be an integer`);
  return raw;
}

function strings(raw: unknown, path: string): string[] {
  if (!Array.isArray(raw) || raw.some((item) => typeof item !== "string")) {
    throw new Error(`rhino-local: ${path} must be an array of strings`);
  }
  assertDenseArray(raw, path);
  return raw.slice() as string[];
}

/**
 * A scope is a set: AM's token holds each scope once, and the verdict compares
 * scopes as sets. A repeated entry is refused rather than collapsed, so an
 * expectation cannot look stricter than it is (Sol ctx r1 P1-1).
 */
function scopeSet(raw: unknown, path: string): string[] {
  const list = strings(raw, path);
  const seen = new Set<string>();
  for (const item of list) {
    if (seen.has(item)) throw new Error(`rhino-local: ${path} lists ${JSON.stringify(item)} twice; a scope is a set`);
    seen.add(item);
  }
  return list;
}

function stringArrayMap(raw: unknown, path: string): Record<string, string[]> {
  const obj = plain(raw, path);
  const out: Record<string, string[]> = {};
  for (const [key, value] of Object.entries(obj)) defineOwn(out, key, strings(value, `${path}.${key}`));
  return out;
}
