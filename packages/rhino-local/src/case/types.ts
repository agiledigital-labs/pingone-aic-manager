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

export const CASE_KEYS = ["name", "script", "outcomes", "given", "expect"] as const;
export type CaseKey = (typeof CASE_KEYS)[number];

export const ENGINES = ["next-gen", "legacy"] as const;
export type Engine = (typeof ENGINES)[number];

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
  "bindingOverrides",
  "identityAttributes",
  "engine",
  "bindings",
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
export type EvidenceChannel = Channel | "nodeState";

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
  reply: { code: number };
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
}

export interface Case {
  name: string;
  script: string;
  outcomes?: readonly string[];
  given: Given;
  expect: Expect;
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
