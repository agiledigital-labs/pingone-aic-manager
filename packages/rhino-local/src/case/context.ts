/**
 * Script contexts: what each one accepts in a case. The runtime half (which
 * bindings exist and how they behave) is the context's overlay under
 * `src/bindings/rhino/`; this file is the validation half, so a case that
 * names a key its context cannot see fails before it runs.
 * packages/rhino-local/docs/script-contexts.md.
 */
import { DEFAULT_SCRIPT_CONTEXT, LIBRARY_CALLER_CONTEXTS, SCRIPT_CONTEXTS } from "./types.ts";
import type {
  AccessTokenExpect,
  AccessTokenSeed,
  AllowUndeclaredChannel,
  ConfigProviderExpect,
  ValidateScopeExpect,
  ValidateScopeSeed,
  MayActExpect,
  LibraryExpect,
  LibrarySeed,
  LibraryCallerContext,
  ClientSeed,
  Engine,
  JsonObject,
  ExpectedValue,
  ExpectKey,
  GivenKey,
  IdentitySeed,
  OidcClaimsExpect,
  OidcClaimsSeed,
  SocialIdpExpect,
  SocialIdpSeed,
  ScriptContext,
  TokenRequestSeed,
} from "./types.ts";
import { assertDenseArray, defineOwn, isPlainObject, parseJsonObject, parseJsonValue, unknownKeyError } from "./util.ts";

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
export const CONTEXT_ONLY_GIVEN: readonly GivenKey[] = [
  "accessToken",
  "identity",
  "client",
  "scopes",
  "request",
  "oidc",
  "socialIdp",
  "validateScope",
  "library",
];
export const CONTEXT_ONLY_EXPECT: readonly ExpectKey[] = ["accessToken", "oidc", "socialIdp", "configProvider", "validateScope", "mayAct", "returns"];
export const CONTEXT_ONLY_ALLOW: readonly AllowUndeclaredChannel[] = [
  "accessToken",
  "oidc",
  "socialIdp",
  "configProvider",
  "validateScope",
  "mayAct",
  "returns",
];

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
  OIDC_CLAIMS: {
    id: "OIDC_CLAIMS",
    engines: ["next-gen", "legacy"],
    givenKeys: [...SHARED_GIVEN, "identity", "client", "scopes", "request", "oidc"],
    // `outcome` only as the `null` a validated case carries (parseExpect).
    expectKeys: ["outcome", "oidc", "openidm", "http", "logs", "allowUndeclared"],
    allowUndeclared: ["oidc", "openidmWrites", "openidmReads", "http", "logs"],
    hasOutcome: false,
  },
  SOCIAL_IDP_PROFILE_TRANSFORMATION: {
    id: "SOCIAL_IDP_PROFILE_TRANSFORMATION",
    engines: ["next-gen", "legacy"],
    // The node state is bound (nodeState on both engines, sharedState and
    // transientState on legacy), so state seeds and diffs belong here.
    givenKeys: [
      ...SHARED_GIVEN,
      "sharedState",
      "transientState",
      "requestHeaders",
      "requestParameters",
      "cookieName",
      "loggerScriptId",
      "socialIdp",
    ],
    expectKeys: ["outcome", "socialIdp", "sharedState", "transientState", "openidm", "http", "logs", "allowUndeclared"],
    allowUndeclared: ["socialIdp", "sharedState", "transientState", "openidmWrites", "openidmReads", "http", "logs"],
    hasOutcome: false,
  },
  CONFIG_PROVIDER_NODE: {
    id: "CONFIG_PROVIDER_NODE",
    // Both engines, measured 2026-11-10: the node's script list offers
    // CONFIG_PROVIDER_NODE (1.0) and CONFIG_PROVIDER_NODE_NEXT_GEN (2.0)
    // scripts, and a journey runs either.
    engines: ["next-gen", "legacy"],
    // nodeState is bound on both engines (sharedState and transientState
    // are not, even on legacy), so state seeds and diffs belong here.
    givenKeys: [
      ...SHARED_GIVEN,
      "sharedState",
      "transientState",
      "requestHeaders",
      "requestParameters",
      "cookieName",
      "loggerScriptId",
    ],
    expectKeys: ["outcome", "configProvider", "sharedState", "transientState", "openidm", "http", "logs", "allowUndeclared"],
    allowUndeclared: ["configProvider", "sharedState", "transientState", "openidmWrites", "openidmReads", "http", "logs"],
    hasOutcome: false,
  },
  OAUTH2_VALIDATE_SCOPE: {
    id: "OAUTH2_VALIDATE_SCOPE",
    // Both engines, measured 2026-11-10: OAUTH2_VALIDATE_SCOPE (1.0) and
    // OAUTH2_VALIDATE_SCOPE_NEXT_GEN (2.0) scripts both run as a client's
    // validateScopeScript override.
    engines: ["next-gen", "legacy"],
    givenKeys: [...SHARED_GIVEN, "loggerScriptId", "cookieName", "client", "request", "validateScope"],
    expectKeys: ["outcome", "validateScope", "openidm", "http", "logs", "allowUndeclared"],
    allowUndeclared: ["validateScope", "openidmWrites", "openidmReads", "http", "logs"],
    hasOutcome: false,
  },
  OAUTH2_MAY_ACT: {
    id: "OAUTH2_MAY_ACT",
    // Both engines, measured 2026-11-10: OAUTH2_MAY_ACT (1.0) and
    // OAUTH2_MAY_ACT_NEXT_GEN (2.0) scripts both run as a client's
    // accessTokenMayActScript. The design note called this next-gen only.
    engines: ["next-gen", "legacy"],
    givenKeys: [...SHARED_GIVEN, "loggerScriptId", "cookieName", "client", "request", "accessToken", "identity", "scopes"],
    expectKeys: ["outcome", "mayAct", "openidm", "http", "logs", "allowUndeclared"],
    allowUndeclared: ["mayAct", "openidmWrites", "openidmReads", "http", "logs"],
    hasOutcome: false,
  },
  LIBRARY: {
    id: "LIBRARY",
    // Next-gen only, measured 2026-11-11. Legacy decision and legacy token
    // modification both throw ReferenceError: "require" is not defined.
    engines: ["next-gen"],
    givenKeys: [...SHARED_GIVEN, "loggerScriptId", "cookieName", "library"],
    // `outcome` only as the `null` a validated case carries (parseExpect).
    expectKeys: ["outcome", "returns", "openidm", "http", "logs", "allowUndeclared"],
    allowUndeclared: ["returns", "openidmWrites", "openidmReads", "http", "logs"],
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

const MAY_ACT_TOKEN_KEYS = ["clientId", "grantType", "realm", "scope", "fields"] as const;

/**
 * The access-token members OAUTH2_MAY_ACT actually reads. Anything else is
 * refused: the overlay would drop it and a case could go green on a fixture
 * the script never sees (Sol ctx r5 P2-5). Only `client_credentials` was
 * measured.
 */
export function parseMayActAccessTokenSeed(raw: unknown, path: string): AccessTokenSeed {
  const obj = plain(raw, path, MAY_ACT_TOKEN_KEYS);
  const out: AccessTokenSeed = {};
  if (obj.clientId !== undefined) out.clientId = str(obj.clientId, `${path}.clientId`);
  if (obj.grantType !== undefined) {
    if (obj.grantType !== "client_credentials") {
      throw new Error(
        `rhino-local: ${path}.grantType must be "client_credentials" until another grant is measured`
      );
    }
    out.grantType = "client_credentials";
  }
  if (obj.realm !== undefined) out.realm = str(obj.realm, `${path}.realm`);
  if (obj.scope !== undefined) out.scope = scopeSet(obj.scope, `${path}.scope`);
  if (obj.fields !== undefined) {
    const fields = plain(obj.fields, `${path}.fields`);
    const parsed: JsonObject = {};
    for (const key of Object.keys(fields)) {
      if (key !== "expires_in") {
        throw new Error(
          `rhino-local: ${path}.fields.${key} is not measured on a may-act token; only expires_in was read`
        );
      }
    }
    if (fields.expires_in !== undefined) {
      if (typeof fields.expires_in !== "number" || !Number.isFinite(fields.expires_in)) {
        throw new Error(`rhino-local: ${path}.fields.expires_in must be a finite number`);
      }
      parsed.expires_in = fields.expires_in;
    }
    out.fields = parsed;
  }
  return out;
}

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

export function parseOidcClaimsSeed(raw: unknown, path: string): OidcClaimsSeed {
  const obj = plain(raw, path, ["sub", "subname", "claimsLocales"]);
  const out: OidcClaimsSeed = {};
  if (obj.sub !== undefined) out.sub = str(obj.sub, `${path}.sub`);
  if (obj.subname !== undefined) out.subname = str(obj.subname, `${path}.subname`);
  if (obj.claimsLocales !== undefined) out.claimsLocales = strings(obj.claimsLocales, `${path}.claimsLocales`);
  return out;
}

/** Keys of {@link OidcClaimsExpect} whose value is an `ExpectedValue`. */
export const OIDC_VALUE_KEYS = ["userinfo", "idToken", "compositeScopes"] as const;

export function parseOidcClaimsExpect(
  raw: unknown,
  path: string,
  expectedValue: (raw: unknown, path: string) => ExpectedValue
): OidcClaimsExpect {
  const obj = plain(raw, path, [...OIDC_VALUE_KEYS, "issued"]);
  const out: OidcClaimsExpect = {};
  for (const key of OIDC_VALUE_KEYS) {
    if (key in obj) out[key] = expectedValue(obj[key], `${path}.${key}`);
  }
  if (obj.issued !== undefined) {
    if (typeof obj.issued !== "boolean") throw new Error(`rhino-local: ${path}.issued must be a boolean`);
    out.issued = obj.issued;
  }
  return out;
}

export function parseSocialIdpSeed(raw: unknown, path: string): SocialIdpSeed {
  const obj = plain(raw, path, ["rawProfile", "selectedIdp"]);
  const out: SocialIdpSeed = {};
  if (obj.rawProfile !== undefined) out.rawProfile = parseJsonObject(obj.rawProfile, `${path}.rawProfile`);
  if (obj.selectedIdp !== undefined) out.selectedIdp = str(obj.selectedIdp, `${path}.selectedIdp`);
  return out;
}

export function parseSocialIdpExpect(
  raw: unknown,
  path: string,
  expectedValue: (raw: unknown, path: string) => ExpectedValue
): SocialIdpExpect {
  const obj = plain(raw, path, ["profile", "accepted"]);
  const out: SocialIdpExpect = {};
  if ("profile" in obj) out.profile = expectedValue(obj.profile, `${path}.profile`);
  if (obj.accepted !== undefined) {
    if (typeof obj.accepted !== "boolean") throw new Error(`rhino-local: ${path}.accepted must be a boolean`);
    out.accepted = obj.accepted;
  }
  return out;
}

export function parseConfigProviderExpect(
  raw: unknown,
  path: string,
  expectedValue: (raw: unknown, path: string) => ExpectedValue
): ConfigProviderExpect {
  const obj = plain(raw, path, ["config", "accepted"]);
  const out: ConfigProviderExpect = {};
  if ("config" in obj) out.config = expectedValue(obj.config, `${path}.config`);
  if (obj.accepted !== undefined) {
    if (typeof obj.accepted !== "boolean") throw new Error(`rhino-local: ${path}.accepted must be a boolean`);
    out.accepted = obj.accepted;
  }
  return out;
}

export function parseValidateScopeSeed(raw: unknown, path: string): ValidateScopeSeed {
  const obj = plain(raw, path, ["requested", "allowed", "available", "defaults"]);
  const out: ValidateScopeSeed = {};
  for (const key of ["requested", "allowed", "available", "defaults"] as const) {
    if (obj[key] !== undefined) out[key] = strings(obj[key], `${path}.${key}`);
  }
  return out;
}

export function parseValidateScopeExpect(raw: unknown, path: string): ValidateScopeExpect {
  const obj = plain(raw, path, ["granted", "invalidScope", "accepted"]);
  const out: ValidateScopeExpect = {};
  if (obj.granted !== undefined) out.granted = strings(obj.granted, `${path}.granted`);
  if (obj.invalidScope !== undefined) out.invalidScope = str(obj.invalidScope, `${path}.invalidScope`);
  if (obj.accepted !== undefined) {
    if (typeof obj.accepted !== "boolean") throw new Error(`rhino-local: ${path}.accepted must be a boolean`);
    out.accepted = obj.accepted;
  }
  if (out.granted !== undefined && (out.invalidScope !== undefined || out.accepted === false)) {
    throw new Error(`rhino-local: ${path}.granted cannot be declared with a refusal`);
  }
  return out;
}

export function parseMayActExpect(raw: unknown, path: string): MayActExpect {
  const obj = plain(raw, path, ["mayAct", "act", "scope", "issued", "refusal"]);
  const out: MayActExpect = {};
  // A present key, including null, is a JSON-null claim. An omitted key
  // requires the claim to be absent. `!== undefined` would collapse the two.
  if ("mayAct" in obj) out.mayAct = parseJsonValue(obj.mayAct, `${path}.mayAct`);
  if ("act" in obj) out.act = parseJsonValue(obj.act, `${path}.act`);
  if (obj.scope !== undefined) out.scope = scopeSet(obj.scope, `${path}.scope`);
  if ("issued" in obj) {
    if (typeof obj.issued !== "boolean") throw new Error(`rhino-local: ${path}.issued must be a boolean`);
    out.issued = obj.issued;
  }
  if (obj.refusal !== undefined) out.refusal = str(obj.refusal, `${path}.refusal`);
  if (out.issued === false && ("mayAct" in out || "act" in out || out.scope !== undefined)) {
    throw new Error(`rhino-local: ${path} cannot declare mayAct, act or scope together with issued: false`);
  }
  return out;
}

export function parseLibrarySeed(raw: unknown, path: string): LibrarySeed {
  const obj = plain(raw, path, ["export", "args", "callerContext"]);
  const name = str(obj.export, `${path}.export`);
  if (name.length === 0) {
    throw new Error(`rhino-local: ${path}.export must name an export`);
  }
  const out: LibrarySeed = { export: name };
  if (obj.args !== undefined) {
    if (!Array.isArray(obj.args)) {
      throw new Error(`rhino-local: ${path}.args must be an array of JSON values`);
    }
    assertDenseArray(obj.args, `${path}.args`);
    out.args = obj.args.map((item, index) => parseJsonValue(item, `${path}.args[${index}]`));
  }
  if (obj.callerContext !== undefined) {
    if (!(LIBRARY_CALLER_CONTEXTS as readonly string[]).includes(String(obj.callerContext))) {
      throw new Error(
        `rhino-local: ${path}.callerContext must be one of ${LIBRARY_CALLER_CONTEXTS.join(", ")}`
      );
    }
    out.callerContext = obj.callerContext as LibraryCallerContext;
  }
  return out;
}

export function parseLibraryExpect(raw: unknown, path: string): LibraryExpect {
  const obj = plain(raw, path, ["status", "phase", "undefined", "value", "type", "name", "message"]);
  if (obj.status !== "returned" && obj.status !== "thrown") {
    throw new Error(`rhino-local: ${path}.status must be "returned" or "thrown"`);
  }
  const out: LibraryExpect = { status: obj.status };
  if (obj.phase !== undefined) {
    if (obj.phase !== "export" && obj.phase !== "module") {
      throw new Error(`rhino-local: ${path}.phase must be "export" or "module"`);
    }
    out.phase = obj.phase;
  }
  if (obj.status === "returned") {
    if (obj.phase === "module") {
      throw new Error(`rhino-local: ${path}.phase "module" is a throw while the library loads, not a return`);
    }
    const flagged = obj.undefined === true;
    if (obj.undefined !== undefined && obj.undefined !== true) {
      throw new Error(`rhino-local: ${path}.undefined must be true`);
    }
    if (flagged === ("value" in obj)) {
      throw new Error(`rhino-local: ${path} must declare value or undefined: true, and not both`);
    }
    if (flagged) {
      out.undefined = true;
    } else {
      out.value = parseJsonValue(obj.value, `${path}.value`);
    }
    return out;
  }
  out.message = str(obj.message, `${path}.message`);
  out.name = str(obj.name, `${path}.name`);
  out.type = str(obj.type, `${path}.type`);
  if (out.phase === undefined) {
    out.phase = "export";
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
