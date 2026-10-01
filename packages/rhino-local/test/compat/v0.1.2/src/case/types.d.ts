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
export declare const CASE_KEYS: readonly ["name", "script", "outcomes", "given", "expect"];
export type CaseKey = (typeof CASE_KEYS)[number];
export declare const ENGINES: readonly ["next-gen", "legacy"];
export type Engine = (typeof ENGINES)[number];
export declare const GIVEN_KEYS: readonly ["sharedState", "transientState", "secureState", "realm", "scriptName", "cookieName", "resumedFromSuspend", "requestHeaders", "requestParameters", "requestCookies", "locales", "existingSession", "esv", "secrets", "libraries", "callbacks", "managed", "http", "engine", "bindings"];
export type GivenKey = (typeof GIVEN_KEYS)[number];
/** Bindings that have a dedicated `given` field; do not also put them in `bindings`. */
export declare const GIVEN_BINDING_SEEDS: readonly ["realm", "scriptName", "cookieName", "resumedFromSuspend", "requestHeaders", "requestParameters", "requestCookies", "locales", "existingSession"];
export declare const EXPECT_KEYS: readonly ["outcome", "sharedState", "transientState", "secureState", "callbacks", "openidm", "http", "logs", "allowUndeclared"];
export type ExpectKey = (typeof EXPECT_KEYS)[number];
export declare const STATE_DIFF_KEYS: readonly ["added", "changed", "removed"];
export declare const ALLOW_UNDECLARED_CHANNELS: readonly ["openidmWrites", "openidmReads", "http", "logs", "callbacks", "sharedState", "transientState", "secureState"];
export type AllowUndeclaredChannel = (typeof ALLOW_UNDECLARED_CHANNELS)[number];
export type AllowUndeclared = {
    [K in AllowUndeclaredChannel]?: boolean;
};
/**
 * Per-channel defaults. Fail-closed on undeclared writes, HTTP, callbacks,
 * and state mutations; fail-open on reads and log lines. Loosening a
 * fail-closed channel requires an explicit `allowUndeclared` flag.
 */
export declare const DEFAULT_ALLOW_UNDECLARED: {
    readonly [K in AllowUndeclaredChannel]: boolean;
};
export declare const OPENIDM_WRITE_METHODS: readonly ["create", "update", "patch", "delete", "action"];
export declare const OPENIDM_READ_METHODS: readonly ["read", "query"];
export declare const OPENIDM_METHODS: readonly ["create", "update", "patch", "delete", "action", "read", "query"];
export type OpenidmWriteMethod = (typeof OPENIDM_WRITE_METHODS)[number];
export type OpenidmReadMethod = (typeof OPENIDM_READ_METHODS)[number];
export type OpenidmMethod = (typeof OPENIDM_METHODS)[number];
export declare const LOG_LEVELS: readonly ["error", "warn", "info", "debug", "trace"];
export type LogLevel = (typeof LOG_LEVELS)[number];
export declare const CHANNELS: readonly ["outcome", "sharedState", "transientState", "secureState", "callbacks", "openidm", "http", "logs"];
/** compat-accepted: the two channels added since 0.1.2 can appear in what the harness reports. */
export type Channel = (typeof CHANNELS)[number] | "sessionProperties" | "identityWrites";
export declare const STATE_CHANNELS: readonly ["sharedState", "transientState", "secureState"];
export type StateChannel = (typeof STATE_CHANNELS)[number];
export type EvidenceChannel = Channel | "nodeState";
export declare const ENV_INPUT_KEYS: readonly ["esv", "secrets", "managed", "http"];
export type EnvInputKey = (typeof ENV_INPUT_KEYS)[number];
export interface StateDiff {
    added?: ExpectedObject;
    changed?: ExpectedObject;
    removed?: string[];
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
    transientState?: JsonObject;
    secureState?: JsonObject;
    realm?: string;
    scriptName?: string;
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
    /** compat-accepted: a `null` declares an absent ESV (README, 0.1.2 migration). */
    esv?: Record<string, string | null>;
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
    callbacks?: CallbackExpect[];
    openidm?: OpenidmExpect[];
    http?: HttpExpect[];
    logs?: LogExpect[];
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
export type StateMutation = {
    operation: "added";
    key: string;
    after: JsonValue;
} | {
    operation: "changed";
    key: string;
    before: JsonValue;
    after: JsonValue;
} | {
    operation: "removed";
    key: string;
    before: JsonValue;
};
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
 * Observed effects of one run. Every channel is required: omitting a
 * fail-closed channel would silently assert nothing, which is the failure
 * mode this harness exists to avoid. The runner fills this in; the verdict
 * engine treats it as data.
 */
export interface RecordedEffects {
    outcome: string | null;
    /** Decision discarded when next-gen callbacks suspend; diagnostic only. */
    discardedOutcome?: string | null;
    sharedState: StateBucket;
    transientState: StateBucket;
    secureState: StateBucket;
    callbacks: CallbackEffect[];
    openidm: OpenidmEffect[];
    http: HttpEffect[];
    logs: LogEffect[];
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
