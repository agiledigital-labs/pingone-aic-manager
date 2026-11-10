/**
 * Runner-agnostic scripted-decision case: one definition drives the local
 * Rhino lane and the AIC wrapper-journey lane. `given` is the seed;
 * `expect` is diffs and per-channel effects, never whole-state equality.
 */

export type JsonPrimitive = string | number | boolean | null;
export type JsonValue = JsonPrimitive | JsonObject | JsonValue[];
export interface JsonObject {
  [key: string]: JsonValue;
}

/** Standard Schema v1, consumed structurally so the case layer never imports zod. */
export interface StandardSchema {
  readonly "~standard": {
    readonly vendor?: string;
    validate(value: unknown): unknown;
  };
}

export type ExpectedValue = JsonPrimitive | RegExp | StandardSchema | ExpectedObject | ExpectedValue[];
export interface ExpectedObject {
  [key: string]: ExpectedValue;
}

export type Pattern = string | RegExp;

export const CASE_KEYS = ["name", "script", "outcomes", "given", "expect", "context"] as const;
export type CaseKey = (typeof CASE_KEYS)[number];

export const ENGINES = ["next-gen", "legacy"] as const;
export type Engine = (typeof ENGINES)[number];

/**
 * The AM script context a case runs in, by AM's context id without the
 * `_NEXT_GEN` suffix (the engine is `given.engine`). Absent means the
 * scripted decision node, which is how every case ran before contexts
 * existed. packages/rhino-local/docs/script-contexts.md.
 */
export const SCRIPT_CONTEXTS = ["SCRIPTED_DECISION_NODE", "OAUTH2_ACCESS_TOKEN_MODIFICATION", "OIDC_CLAIMS"] as const;
export type ScriptContext = (typeof SCRIPT_CONTEXTS)[number];
export const DEFAULT_SCRIPT_CONTEXT: ScriptContext = "SCRIPTED_DECISION_NODE";

export const GIVEN_KEYS = [
  "sharedState",
  "registeredObjectAttributes",
  "transientState",
  "secureState",
  "realm",
  "scriptName",
  "loggerScriptId",
  "cookieName",
  "resumedFromSuspend",
  "requestHeaders",
  "requestParameters",
  "requestCookies",
  "locales",
  "existingSession",
  "esv",
  "esvUndeclared",
  "secrets",
  "libraries",
  "callbacks",
  "managed",
  "http",
  "openidmActions",
  "openidmFailures",
  "openidmPriorCalls",
  "identityCustomAttrs",
  "identityCustomAttrsOwnedKeys",
  "bindingOverrides",
  "identityAttributes",
  "engine",
  "bindings",
  "accessToken",
  "identity",
  "client",
  "scopes",
  "request",
  "oidc",
] as const;
export type GivenKey = (typeof GIVEN_KEYS)[number];

/** Bindings that have a dedicated `given` field; do not also put them in `bindings`. */
export const GIVEN_BINDING_SEEDS = [
  "realm",
  "scriptName",
  "cookieName",
  "resumedFromSuspend",
  "requestHeaders",
  "requestParameters",
  "requestCookies",
  "locales",
  "existingSession",
] as const;

export const EXPECT_KEYS = [
  "outcome",
  "sharedState",
  "transientState",
  "secureState",
  "sessionProperties",
  "callbacks",
  "openidm",
  "http",
  "logs",
  "identityWrites",
  "allowUndeclared",
  "accessToken",
  "oidc",
] as const;
export type ExpectKey = (typeof EXPECT_KEYS)[number];

export const STATE_DIFF_KEYS = ["added", "changed", "removed"] as const;

export const ALLOW_UNDECLARED_CHANNELS = [
  "openidmWrites",
  "openidmReads",
  "http",
  "logs",
  "callbacks",
  "sharedState",
  "transientState",
  "secureState",
  "sessionProperties",
  "identityWrites",
  "accessToken",
  "oidc",
] as const;
export type AllowUndeclaredChannel = (typeof ALLOW_UNDECLARED_CHANNELS)[number];

export type AllowUndeclared = {
  [K in AllowUndeclaredChannel]?: boolean;
};

/**
 * Per-channel defaults when the expectation omits the channel. Fail-closed
 * on undeclared writes (OpenIDM and identity), HTTP, callbacks, and state
 * mutations; fail-open on
 * OpenIDM reads and log lines. Declaring `openidm` or `logs` makes those
 * channels exhaustive unless `allowUndeclared` explicitly loosens them.
 */
export const DEFAULT_ALLOW_UNDECLARED: {
  readonly [K in AllowUndeclaredChannel]: boolean;
} = {
  openidmWrites: false,
  openidmReads: true,
  http: false,
  logs: true,
  callbacks: false,
  sharedState: false,
  transientState: false,
  secureState: false,
  sessionProperties: false,
  identityWrites: false,
  accessToken: false,
  oidc: false,
};

export const OPENIDM_WRITE_METHODS = [
  "create",
  "update",
  "patch",
  "delete",
  "action",
] as const;
export const OPENIDM_READ_METHODS = ["read", "query"] as const;
export const OPENIDM_METHODS = [
  ...OPENIDM_WRITE_METHODS,
  ...OPENIDM_READ_METHODS,
] as const;
export type OpenidmWriteMethod = (typeof OPENIDM_WRITE_METHODS)[number];
export type OpenidmReadMethod = (typeof OPENIDM_READ_METHODS)[number];
export type OpenidmMethod = (typeof OPENIDM_METHODS)[number];

export const LOG_LEVELS = ["error", "warn", "info", "debug", "trace"] as const;
export type LogLevel = (typeof LOG_LEVELS)[number];

export const CHANNELS = [
  "outcome",
  "sharedState",
  "transientState",
  "secureState",
  "sessionProperties",
  "callbacks",
  "openidm",
  "http",
  "logs",
  "identityWrites",
] as const;
export type Channel = (typeof CHANNELS)[number];

export const STATE_CHANNELS = [
  "sharedState",
  "transientState",
  "secureState",
] as const;
export type StateChannel = (typeof STATE_CHANNELS)[number];
/** `accessToken` and `oidc` are judged only for their context, so neither is a `Channel` every runner must record. */
export type EvidenceChannel = Channel | "nodeState" | "accessToken" | "oidc";

export const ENV_INPUT_KEYS = ["esv", "secrets", "managed", "http", "openidmActions", "openidmFailures", "bindingOverrides"] as const;
export type EnvInputKey = (typeof ENV_INPUT_KEYS)[number];

export interface StateDiff {
  added?: ExpectedObject;
  changed?: ExpectedObject;
  removed?: string[];
}

/** One declared AM attribute -> IDM property layout for local `identity.store()`. */
export interface IdentityAttributeMapping {
  /** The IDM managed-object property the AM attribute is stored in. */
  field: string;
  /**
   * `"single"`: one value is stored as a scalar and none removes the property.
   * `"multi"`: always an array, `[]` for none.
   */
  cardinality: "single" | "multi";
}

/** Local policy for undeclared systemEnv properties; default: error. */
export type EsvUndeclared = "error" | "absent";

export interface OpenidmActionMatch {
  resource: Pattern;
  action: string;
}

export interface OpenidmActionReply {
  body: JsonValue;
}

export interface OpenidmActionStub {
  match: OpenidmActionMatch;
  reply: OpenidmActionReply;
}

export interface HttpMatch {
  url: Pattern;
  method?: string;
}

export interface HttpReply {
  status: number;
  body?: JsonValue;
  headers?: Record<string, string>;
}

export interface HttpStub {
  match: HttpMatch;
  reply: HttpReply;
}

/** One numbered call among calls with the same method and resource. */
export interface OpenidmFailureStub {
  match: { method: OpenidmMethod; resource: Pattern; ordinal: number };
  /**
   * `detail` is what `e.javaException.getDetail()` wraps, as AIC's CREST
   * `JsonValue` (measured 2026-11-08: a policy 403 carries the
   * `failedPolicyRequirements` object, a 404 and a 400 carry null). Omitted,
   * reading the detail is a harness fault. `message` replaces the harness's
   * own text after `ResourceExceptionScriptAdapter: `.
   */
  reply: { code: number; detail?: JsonValue; message?: string };
}

export interface HttpExpect {
  url: Pattern;
  method?: string;
  body?: ExpectedValue;
  times?: number;
}

export interface HttpEffect {
  url: string;
  method: string;
  body?: JsonValue;
}

export interface OpenidmExpect {
  method: OpenidmMethod;
  resource: Pattern;
  body?: ExpectedValue;
  actionName?: Pattern;
  times?: number;
}

export interface OpenidmEffect {
  method: OpenidmMethod;
  resource: string;
  body?: JsonValue;
  actionName?: string;
}

/**
 * One attribute an `idRepository.getIdentity(id)` handle persisted with
 * `store()`. Named as the script named it (AM's attribute name), because the
 * write goes through AM's identity repository rather than the `openidm`
 * binding, and the managed-record shape it lands as is unmeasured.
 */
export interface IdentityWriteEffect {
  identity: string;
  attribute: string;
  values: string[];
}

export interface IdentityWriteExpect {
  identity: Pattern;
  attribute: Pattern;
  values?: ExpectedValue;
  times?: number;
}

export interface LogExpect {
  level?: LogLevel;
  message: Pattern;
  times?: number;
}

export interface LogEffect {
  level: LogLevel;
  message: string;
}

export interface CallbackEffect {
  type: string;
  [key: string]: JsonValue;
}

export interface CallbackExpect {
  type: string;
  [key: string]: ExpectedValue;
}

export interface Given {
  sharedState?: JsonObject;
  /** Seed objectAttributes through AM's registered-container merge path. */
  registeredObjectAttributes?: JsonObject;
  transientState?: JsonObject;
  secureState?: JsonObject;
  realm?: string;
  scriptName?: string;
  /** Mock metadata for decision-node `logger.getName()`; not a JS binding. */
  loggerScriptId?: string;
  cookieName?: string;
  resumedFromSuspend?: boolean;
  requestHeaders?: Record<string, string[]>;
  requestParameters?: Record<string, string[]>;
  requestCookies?: Record<string, string>;
  locales?: JsonObject;
  /**
   * `existingSession`. Present only when the request carries a session cookie,
   * and then a String->String map: AM's own session properties plus anything a
   * prior journey stored with `putSessionProperty`. Measured 2026-09-14 on both
   * evaluators (docs/api/12-script-bindings-matrix.md).
   */
  existingSession?: Record<string, string>;
  /** A null entry explicitly declares an absent ESV. */
  esv?: Record<string, string | null>;
  /** Treat undeclared properties as absent on AM, or throw (the default). */
  esvUndeclared?: EsvUndeclared;
  secrets?: Record<string, string>;
  /** Library source keyed by the name passed to next-gen `require()`. */
  libraries?: Record<string, string>;
  /**
   * Submitted callback values a resumed script reads via `callbacks.getXCallbacks()`.
   * Omit to leave the binding unseeded (a read throws naming `given.callbacks`);
   * seed `[]` for a first pass.
   */
  callbacks?: CallbackEffect[];
  managed?: Record<string, JsonObject[]>;
  http?: HttpStub[];
  openidmActions?: OpenidmActionStub[];
  openidmFailures?: OpenidmFailureStub[];
  /**
   * `openidm` calls already made earlier in the journey, counted per
   * `"<method> <resource>"`. The step runner seeds it so `openidmFailures`
   * ordinals number calls across callback passes, not within each pass.
   */
  openidmPriorCalls?: Record<string, number>;
  /** Local AM bag source of truth, keyed by full managed resource path; not judged. */
  identityCustomAttrs?: Record<string, string[]>;
  /** Historical bag-owned keys, retained across value clears; local carry metadata. */
  identityCustomAttrsOwnedKeys?: Record<string, string[]>;
  /** Local binding replacements, as JavaScript expressions keyed by binding name. */
  bindingOverrides?: Record<string, string>;
  /**
   * How the local `identity.store()` lays out AM attributes the harness has
   * no measurement for, keyed by AM attribute name. A declaration overrides
   * a measured default. Local only: the AIC lane writes through the tenant's
   * real mapping, so a declaration never makes a case AIC-ineligible, and a
   * wrong one shows up as a conformance disagreement.
   */
  identityAttributes?: Record<string, IdentityAttributeMapping>;
  engine?: Engine;
  /**
   * Extra binding seeds keyed by generated mock binding name. Unknown names
   * fail validation — a typo must not silently seed nothing. The runtime
   * implements one today, `journey` (`name`, `identityResource`); any other
   * name throws when the case runs.
   */
  bindings?: Record<string, JsonValue>;
  /** `OAUTH2_ACCESS_TOKEN_MODIFICATION`: the token as AM hands it to the script. */
  accessToken?: AccessTokenSeed;
  /** `OAUTH2_ACCESS_TOKEN_MODIFICATION` and `OIDC_CLAIMS`: the resource owner's identity. */
  identity?: IdentitySeed;
  /** `OAUTH2_ACCESS_TOKEN_MODIFICATION` and `OIDC_CLAIMS`: `clientProperties`. */
  client?: ClientSeed;
  /**
   * `OAUTH2_ACCESS_TOKEN_MODIFICATION` and `OIDC_CLAIMS`: the `scopes`
   * binding; defaults to the token's scope, or `["openid"]` for OIDC claims.
   */
  scopes?: string[];
  /** `OAUTH2_ACCESS_TOKEN_MODIFICATION` and `OIDC_CLAIMS`: `requestProperties`. */
  request?: TokenRequestSeed;
  /** `OIDC_CLAIMS`: the `claims` binding's subject and `claimsLocales`. */
  oidc?: OidcClaimsSeed;
}

/**
 * What AM hands an OIDC claims script beyond the shared bindings. Defaults
 * are a password-grant token request, as measured on 2026-11-10
 * (docs/api/12-script-bindings-matrix.md, "OIDC claims").
 */
export interface OidcClaimsSeed {
  /** The `claims` binding's `sub` and AM's id-token subject. Default `given.identity.name`. */
  sub?: string;
  /** The `claims` binding's `subname`. Default `sub`. */
  subname?: string;
  /** The `claims_locales` request parameter, split. Default `[]`. */
  claimsLocales?: string[];
}

/**
 * What the case asserts about the claims an OIDC claims script returns.
 * Fail-closed: returned claims the case does not declare are a mismatch
 * unless `allowUndeclared.oidc` is set.
 */
export interface OidcClaimsExpect {
  /** The userinfo response: the returned `values` verbatim, `null` kept. `{}` when the script returned none. */
  userinfo?: ExpectedValue;
  /**
   * The id token's script-derived claims: the non-null `values`, with `sub`
   * and `subname` as AM resolves them (see {@link OidcClaimsEffect.idToken}).
   * AM's own claims (`aud`, `iss`, `exp`, ...) are not included.
   */
  idToken?: ExpectedValue;
  /** The returned `compositeScopes`, `null` when absent. Measured to filter nothing. */
  compositeScopes?: ExpectedValue;
  /**
   * `false` when AM would fail the token request although the script
   * returned (measured: a legacy script whose value is not a
   * `UserInfoClaims`). Default `true`.
   */
  issued?: boolean;
}

/** The OIDC claims context's result. */
export interface OidcClaimsEffect {
  /** `"none"` when AM takes no script claims (undefined, or an object with no `values`). */
  returned: "claims" | "none";
  /** What the userinfo endpoint answers with: the returned values verbatim, `{}` for none. */
  userinfo: JsonObject;
  /**
   * The id token's script-derived claims. `sub` and `subname` come from the
   * returned values when they hold `sub` (and `subname` only if returned),
   * otherwise from `given.oidc`; every other non-null value is copied.
   */
  idToken: JsonObject;
  compositeScopes: JsonValue;
  /** `null` when issued; otherwise AM's `error_description`. */
  refusal: string | null;
}

/**
 * The access token at the moment AM runs the modification script. Every key
 * is optional; the defaults are a `client_credentials` token as measured on
 * 2026-11-10 (docs/api/12-script-bindings-matrix.md).
 */
export interface AccessTokenSeed {
  /** Default `"test-client"`. */
  clientId?: string;
  /** Default `clientId` — what a `client_credentials` grant has. */
  resourceOwnerId?: string;
  /** Default `"client_credentials"`. */
  grantType?: string;
  /** Default `given.realm`, else `"/alpha"`. */
  realm?: string;
  /** Default `[]`. The granted scope, as `getScope()` and the `scope` claim show it. */
  scope?: string[];
  /** Default `[clientId]`. */
  audience?: string[];
  /** Default `"access_token"`. */
  tokenName?: string;
  /** Default `"Bearer"`. */
  tokenType?: string;
  /** Epoch seconds. Default: the time the case starts. */
  authTimeSeconds?: number;
  /** Epoch milliseconds. Default `authTimeSeconds * 1000 + 3600000`. */
  expiryTime?: number;
  /**
   * The custom fields (`getCustomFields()`, the extra JWT claims). Default
   * `{ subname: resourceOwnerId, expires_in: 3599 }`, as measured on a
   * one-hour token.
   */
  fields?: JsonObject;
  nonce?: string | null;
  authLevel?: number | null;
  claims?: string | null;
  /** Default placeholders; AIC's are per token. */
  tokenId?: string;
  auditTrackingId?: string;
  authGrantId?: string;
}

/** The identity bound as `identity`. Default: the client's own agent profile. */
export interface IdentitySeed {
  /** Default the token's `resourceOwnerId`. */
  name?: string;
  /** Default `id=<name>,ou=agentonly,o=<realm>,ou=services,ou=am-config` (measured for a client). */
  universalId?: string;
  /** Default `true`. */
  exists?: boolean;
  /** Attribute values by name; an unlisted name reads as an empty collection, as measured. */
  attributes?: Record<string, string[]>;
}

/** `clientProperties`. */
export interface ClientSeed {
  /** Default the token's `clientId`. */
  clientId?: string;
  /** Screaming case, as AM gives them: default `["CLIENT_CREDENTIALS"]`. */
  allowedGrantTypes?: string[];
  /** Default the token's scope. */
  allowedScopes?: string[];
  /** Default `[]`. */
  allowedResponseTypes?: string[];
  /** Default `{}`. */
  customProperties?: Record<string, string>;
  /** Next-gen only (legacy does not list it). Default `[]`. */
  redirectUris?: string[];
}

/** `requestProperties`: the token request as AM received it. */
export interface TokenRequestSeed {
  /** Default `{ grant_type, scope, client_id }` from the token. */
  params?: Record<string, string[]>;
  /** Looked up case-insensitively, as measured. Default `{}`. */
  headers?: Record<string, string[]>;
  /** Default `https://tenant.example.com/am/oauth2/realms/root/realms/<realm>/access_token`. */
  uri?: string;
}

/**
 * What the case asserts about the token the script leaves behind. Fail-closed:
 * a change the case does not declare is a mismatch unless
 * `allowUndeclared.accessToken` is set.
 */
export interface AccessTokenExpect {
  /** Custom fields (JWT claims) added, changed or removed, against `given.accessToken.fields`. */
  fields?: StateDiff;
  /** Fields `addExtraData` / `addExtraJsonData` put in the token endpoint's JSON response. */
  responseFields?: StateDiff;
  /** The final scope, compared as a set (AM's order is a hash order). */
  scope?: string[];
  /** The final expiry, epoch milliseconds. */
  expiryTime?: ExpectedValue;
  nonce?: ExpectedValue;
  authLevel?: ExpectedValue;
  claims?: ExpectedValue;
  /** Next-gen only: `setAct`, `setMayAct`, `setPermissions`. */
  act?: ExpectedValue;
  mayAct?: ExpectedValue;
  permissions?: ExpectedValue;
  /**
   * `false` when AM would refuse to issue the token although the script
   * returned (measured: after `removeScopes()`, a `500 server_error`). Default
   * `true`.
   */
  issued?: boolean;
}

/** The token as the harvest reports it, before and after the script. */
export interface AccessTokenState {
  fields: JsonObject;
  responseFields: JsonObject;
  scope: string[];
  expiryTime: number;
  nonce: JsonValue;
  authLevel: JsonValue;
  claims: JsonValue;
  act: JsonValue;
  mayAct: JsonValue;
  permissions: JsonValue;
}

export interface AccessTokenEffect {
  initial: AccessTokenState;
  final: AccessTokenState;
  /** `null` when issued; otherwise AM's `error_description`. */
  refusal: string | null;
}

export interface Expect {
  /**
   * The outcome the script must reach, or `null` for "must not decide" — a
   * pass that queued callbacks and suspended. A suspended pass is a real
   * expectation, not an absence of one: measured 2026-09-14, a next-gen node
   * that sends callbacks returns no outcome at all, and the only way to say so
   * without `null` is to name an outcome the script never produces.
   */
  outcome: string | null;
  sharedState?: StateDiff;
  transientState?: StateDiff;
  secureState?: StateDiff;
  sessionProperties?: StateDiff;
  callbacks?: CallbackExpect[];
  openidm?: OpenidmExpect[];
  http?: HttpExpect[];
  logs?: LogExpect[];
  identityWrites?: IdentityWriteExpect[];
  allowUndeclared?: AllowUndeclared;
  /** `OAUTH2_ACCESS_TOKEN_MODIFICATION` only. */
  accessToken?: AccessTokenExpect;
  /** `OIDC_CLAIMS` only. */
  oidc?: OidcClaimsExpect;
}

export interface CaseInit {
  name: string;
  script: string;
  /**
   * The outcome vocabulary this script may produce. Optional here, required
   * to run on a tenant: AM answers an undeclared outcome with a bare
   * `401 Login failure` and no callback, byte-identical to a compile error,
   * so the wrapper journey has to declare every outcome up front and the
   * only way to be sure it did is for the case to say what they are.
   *
   * Declaring it also moves the typo from an opaque tenant 401 to a local
   * failure in under a second, which is why the local lane enforces it too.
   */
  outcomes?: readonly string[];
  given?: Given;
  expect: Expect;
  /** Absent: the scripted decision node. */
  context?: "SCRIPTED_DECISION_NODE";
}

/**
 * A case for a context other than the decision node. `expect.outcome` does
 * not exist there; each context accepts its own `given` and `expect` keys,
 * and validation refuses the rest.
 */
export interface AccessTokenCaseInit {
  name: string;
  script: string;
  context: "OAUTH2_ACCESS_TOKEN_MODIFICATION";
  given?: Pick<
    Given,
    | "engine"
    | "realm"
    | "scriptName"
    | "esv"
    | "esvUndeclared"
    | "secrets"
    | "libraries"
    | "managed"
    | "http"
    | "openidmActions"
    | "openidmFailures"
    | "bindingOverrides"
    | "accessToken"
    | "identity"
    | "client"
    | "scopes"
    | "request"
  >;
  expect: Pick<Expect, "accessToken" | "openidm" | "http" | "logs" | "allowUndeclared">;
}

/** An `OIDC_CLAIMS` case: the script's value is the claims AM emits. */
export interface OidcClaimsCaseInit {
  name: string;
  script: string;
  context: "OIDC_CLAIMS";
  given?: Pick<
    Given,
    | "engine"
    | "realm"
    | "scriptName"
    | "esv"
    | "esvUndeclared"
    | "secrets"
    | "libraries"
    | "managed"
    | "http"
    | "openidmActions"
    | "openidmFailures"
    | "bindingOverrides"
    | "identity"
    | "client"
    | "scopes"
    | "request"
    | "oidc"
  >;
  expect: Pick<Expect, "oidc" | "openidm" | "http" | "logs" | "allowUndeclared">;
}

export type AnyCaseInit = CaseInit | AccessTokenCaseInit | OidcClaimsCaseInit;

export interface Case {
  name: string;
  script: string;
  outcomes?: readonly string[];
  given: Given;
  expect: Expect;
  /** Absent: the scripted decision node. */
  context?: ScriptContext;
}

export interface StateBucket {
  initial: JsonObject;
  final: JsonObject;
}

export type StateMutation =
  | { operation: "added"; key: string; after: JsonValue }
  | {
      operation: "changed";
      key: string;
      before: JsonValue;
      after: JsonValue;
    }
  | { operation: "removed"; key: string; before: JsonValue };

export type UnbucketedStateMutation = StateMutation & {
  possibleBuckets: StateChannel[];
};

/** What a runner could establish in addition to the effect values themselves. */
export interface RecordingEvidence {
  /** `unified` means per-bucket absence and hidden lower-precedence writes are unknowable. */
  stateBuckets: "exact" | "unified";
  /** Unified state present before the subject but absent from `given`. */
  ambientState: JsonObject;
  /** Unified state mutations whose concrete bucket could not be observed. */
  unbucketedState: UnbucketedStateMutation[];
  /** An empty effect value in one of these channels is not an observation of absence. */
  unobservedChannels: Channel[];
}

/**
 * Observed effects of one run, as a runner hands them in. Every channel 0.1.2
 * had is required: omitting a fail-closed channel would silently assert
 * nothing, which is the failure mode this harness exists to avoid. The two
 * channels added since (`sessionProperties`, `identityWrites`) are optional
 * so a 0.1.2 producer still compiles; an absent one is judged **unobserved**,
 * never empty, because 0.1.2 accepted those calls without recording them.
 * What the harness hands back is `CompleteRecordedEffects`.
 */
export interface RecordedEffects {
  outcome: string | null;
  /** Decision discarded when next-gen callbacks suspend; diagnostic only. */
  discardedOutcome?: string | null;
  sharedState: StateBucket;
  transientState: StateBucket;
  secureState: StateBucket;
  sessionProperties?: StateBucket | undefined;
  callbacks: CallbackEffect[];
  openidm: OpenidmEffect[];
  http: HttpEffect[];
  logs: LogEffect[];
  identityWrites?: IdentityWriteEffect[] | undefined;
  /** The token context's result; absent for every other context. */
  accessToken?: AccessTokenEffect | undefined;
  /** The OIDC claims context's result; absent for every other context. */
  oidc?: OidcClaimsEffect | undefined;
  /** Omitted by exact recorders; present when a lane has qualified evidence. */
  evidence?: RecordingEvidence;
  /**
   * The mock `openidm` store as it stood when the script finished. Evidence
   * for the post-test residue check, deliberately NOT a judged channel — it
   * answers "did anything survive that the harness did not create", which is
   * a property of the test's housekeeping rather than of the script's
   * behaviour. Only the local lane can produce it; AIC has no way to observe
   * the tenant's whole store, and pretending otherwise would put an empty
   * object where "unknown" belongs.
   */
  managedStore?: Record<string, JsonObject[]>;
  /** Local AM bag source of truth, keyed by full managed resource path; not judged. */
  identityCustomAttrs?: Record<string, string[]>;
  /** Historical bag-owned keys, retained across value clears; local carry metadata. */
  identityCustomAttrsOwnedKeys?: Record<string, string[]>;
}

/**
 * Effects with every channel present, as the harness's own recorders and
 * `normaliseEffects` produce them. A channel a producer omitted is empty here
 * and listed in `evidence.unobservedChannels`.
 */
export interface CompleteRecordedEffects extends RecordedEffects {
  sessionProperties: StateBucket;
  identityWrites: IdentityWriteEffect[];
}

export interface Mismatch {
  channel: EvidenceChannel;
  path: string;
  expected: string;
  actual: string;
  message: string;
}

export interface Unverified {
  channel: EvidenceChannel;
  path: string;
  message: string;
}

export interface Verdict {
  pass: boolean;
  /** False when no contradiction was seen but one or more assertions were unobservable. */
  conclusive: boolean;
  portable: boolean;
  mismatches: Mismatch[];
  unverified: Unverified[];
  /** Multi-line explanation; empty string when `pass` is true. */
  summary: string;
}
