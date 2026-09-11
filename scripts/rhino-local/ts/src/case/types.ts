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

export type Pattern = string | RegExp;

export const CASE_KEYS = ["name", "script", "given", "expect"] as const;
export type CaseKey = (typeof CASE_KEYS)[number];

export const ENGINES = ["next-gen", "legacy"] as const;
export type Engine = (typeof ENGINES)[number];

export const GIVEN_KEYS = [
  "sharedState",
  "transientState",
  "secureState",
  "realm",
  "scriptName",
  "cookieName",
  "resumedFromSuspend",
  "requestHeaders",
  "requestParameters",
  "requestCookies",
  "locales",
  "esv",
  "secrets",
  "managed",
  "http",
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
] as const;

export const EXPECT_KEYS = [
  "outcome",
  "sharedState",
  "transientState",
  "secureState",
  "callbacks",
  "openidm",
  "http",
  "logs",
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
] as const;
export type AllowUndeclaredChannel = (typeof ALLOW_UNDECLARED_CHANNELS)[number];

export type AllowUndeclared = {
  [K in AllowUndeclaredChannel]?: boolean;
};

/**
 * Per-channel defaults. Fail-closed on undeclared writes, HTTP, callbacks,
 * and state mutations; fail-open on reads and log lines. Loosening a
 * fail-closed channel requires an explicit `allowUndeclared` flag.
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
  "callbacks",
  "openidm",
  "http",
  "logs",
] as const;
export type Channel = (typeof CHANNELS)[number];

export const ENV_INPUT_KEYS = ["esv", "secrets", "managed", "http"] as const;
export type EnvInputKey = (typeof ENV_INPUT_KEYS)[number];

export interface StateDiff {
  added?: JsonObject;
  changed?: JsonObject;
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
  body?: JsonValue;
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
  esv?: Record<string, string>;
  secrets?: Record<string, string>;
  managed?: Record<string, JsonObject[]>;
  http?: HttpStub[];
  engine?: Engine;
  /**
   * Extra binding seeds keyed by generated mock binding name. Unknown names
   * fail validation — a typo must not silently seed nothing.
   */
  bindings?: Record<string, JsonValue>;
}

export interface Expect {
  outcome: string;
  sharedState?: StateDiff;
  transientState?: StateDiff;
  secureState?: StateDiff;
  callbacks?: CallbackEffect[];
  openidm?: OpenidmExpect[];
  http?: HttpExpect[];
  logs?: LogExpect[];
  allowUndeclared?: AllowUndeclared;
}

export interface CaseInit {
  name: string;
  script: string;
  given?: Given;
  expect: Expect;
}

export interface Case {
  name: string;
  script: string;
  given: Given;
  expect: Expect;
}

export interface StateBucket {
  initial: JsonObject;
  final: JsonObject;
}

/**
 * Observed effects of one run. Every channel is required: omitting a
 * fail-closed channel would silently assert nothing, which is the failure
 * mode this harness exists to avoid. The runner fills this in; the verdict
 * engine treats it as data.
 */
export interface RecordedEffects {
  outcome: string | null;
  sharedState: StateBucket;
  transientState: StateBucket;
  secureState: StateBucket;
  callbacks: CallbackEffect[];
  openidm: OpenidmEffect[];
  http: HttpEffect[];
  logs: LogEffect[];
}

export interface Mismatch {
  channel: Channel;
  path: string;
  expected: string;
  actual: string;
  message: string;
}

export interface Verdict {
  pass: boolean;
  portable: boolean;
  mismatches: Mismatch[];
  /** Multi-line explanation; empty string when `pass` is true. */
  summary: string;
}
