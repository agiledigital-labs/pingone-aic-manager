import { loadContext } from "../load.ts";
import { bindingsJsonPath } from "../paths.ts";
import {
  ALLOW_UNDECLARED_CHANNELS,
  CASE_KEYS,
  ENGINES,
  EXPECT_KEYS,
  GIVEN_BINDING_SEEDS,
  GIVEN_KEYS,
  LOG_LEVELS,
  OPENIDM_METHODS,
  STATE_DIFF_KEYS,
} from "./types.ts";
import type {
  AllowUndeclared,
  AllowUndeclaredChannel,
  CallbackEffect,
  Case,
  CaseInit,
  Engine,
  Expect,
  Given,
  HttpExpect,
  HttpMatch,
  HttpReply,
  HttpStub,
  JsonObject,
  JsonValue,
  LogExpect,
  LogLevel,
  OpenidmExpect,
  OpenidmMethod,
  Pattern,
  StateDiff,
} from "./types.ts";
import { isPlainObject, parseJsonObject, parseJsonValue, unknownKeyError } from "./util.ts";

const GIVEN_BINDING_SEED_SET: ReadonlySet<string> = new Set(GIVEN_BINDING_SEEDS);
const OPENIDM_METHOD_SET: ReadonlySet<string> = new Set(OPENIDM_METHODS);
const LOG_LEVEL_SET: ReadonlySet<string> = new Set(LOG_LEVELS);
const ENGINE_SET: ReadonlySet<string> = new Set(ENGINES);
const ALLOW_UNDECLARED_SET: ReadonlySet<string> = new Set(
  ALLOW_UNDECLARED_CHANNELS
);

let cachedBindingNames: ReadonlySet<string> | undefined;

function knownBindingNames(): ReadonlySet<string> {
  if (cachedBindingNames === undefined) {
    cachedBindingNames = new Set(
      loadContext(bindingsJsonPath).bindings.map((binding) => binding.name)
    );
  }
  return cachedBindingNames;
}

/** Typed entry point for case authors. */
export function defineCase(input: CaseInit): Case {
  return validateCase(input);
}

/**
 * Runtime validation. Dynamic cases (and typos TypeScript never sees) must
 * fail here rather than assert nothing.
 */
export function validateCase(input: unknown): Case {
  if (!isPlainObject(input)) {
    throw new Error("rhino-local: case is not an object");
  }
  rejectUnknownKeys("case", input, CASE_KEYS);
  const name = parseNonEmptyString(input.name, "case.name");
  const script = parseNonEmptyString(input.script, "case.script");
  const given =
    input.given === undefined ? {} : parseGiven(input.given, "case.given");
  if (!("expect" in input)) {
    throw new Error("rhino-local: case.expect is required");
  }
  const expect = parseExpect(input.expect, "case.expect");
  const kase: Case = { name, script, given, expect };
  if (input.outcomes !== undefined) {
    kase.outcomes = parseOutcomes(input.outcomes, "case.outcomes", expect.outcome);
  }
  return kase;
}

/**
 * The declared outcome vocabulary. Empty is rejected rather than treated as
 * "no opinion" — `outcomes: []` reads like a deliberate statement and would
 * otherwise silently disable the check it was written to request.
 */
function parseOutcomes(
  raw: unknown,
  path: string,
  expected: string | null
): readonly string[] {
  if (!Array.isArray(raw)) {
    throw new Error(`rhino-local: ${path} must be an array of strings`);
  }
  if (raw.length === 0) {
    throw new Error(
      `rhino-local: ${path} must not be empty; omit the key entirely to decline to declare`
    );
  }
  const seen = new Set<string>();
  for (const [index, value] of raw.entries()) {
    if (typeof value !== "string" || value.trim() === "") {
      throw new Error(`rhino-local: ${path}[${index}] must be a non-empty string`);
    }
    if (seen.has(value)) {
      throw new Error(`rhino-local: ${path} lists ${JSON.stringify(value)} twice`);
    }
    seen.add(value);
  }
  // A suspended pass reaches no outcome, so it cannot be checked against the
  // vocabulary — and the vocabulary still matters, because the wrapper journey
  // has to declare every outcome the LATER passes may produce.
  if (expected !== null && !seen.has(expected)) {
    throw new Error(
      `rhino-local: case.expect.outcome ${JSON.stringify(expected)} is not in ${path} [${raw.join(", ")}] — the case expects an outcome the script is not declared to produce`
    );
  }
  return raw.slice() as readonly string[];
}

function parseGiven(raw: unknown, path: string): Given {
  if (!isPlainObject(raw)) {
    throw new Error(`rhino-local: ${path} is not an object`);
  }
  rejectUnknownKeys(path, raw, GIVEN_KEYS);
  const given: Given = {};
  assignJsonObject(given, "sharedState", raw.sharedState, `${path}.sharedState`);
  assignJsonObject(
    given,
    "transientState",
    raw.transientState,
    `${path}.transientState`
  );
  assignJsonObject(given, "secureState", raw.secureState, `${path}.secureState`);
  assignString(given, "realm", raw.realm, `${path}.realm`);
  assignString(given, "scriptName", raw.scriptName, `${path}.scriptName`);
  assignString(given, "cookieName", raw.cookieName, `${path}.cookieName`);
  if (raw.resumedFromSuspend !== undefined) {
    if (typeof raw.resumedFromSuspend !== "boolean") {
      throw new Error(`rhino-local: ${path}.resumedFromSuspend must be a boolean`);
    }
    given.resumedFromSuspend = raw.resumedFromSuspend;
  }
  assignStringArrayMap(
    given,
    "requestHeaders",
    raw.requestHeaders,
    `${path}.requestHeaders`
  );
  assignStringArrayMap(
    given,
    "requestParameters",
    raw.requestParameters,
    `${path}.requestParameters`
  );
  assignStringMap(
    given,
    "requestCookies",
    raw.requestCookies,
    `${path}.requestCookies`
  );
  assignJsonObject(given, "locales", raw.locales, `${path}.locales`);
  // String->String, because that is what AM stores: every value in the measured
  // 23-key session was a string, `AuthLevel: "0"` included.
  assignStringMap(
    given,
    "existingSession",
    raw.existingSession,
    `${path}.existingSession`
  );
  assignStringMap(given, "esv", raw.esv, `${path}.esv`);
  assignStringMap(given, "secrets", raw.secrets, `${path}.secrets`);
  if (raw.callbacks !== undefined) {
    given.callbacks = parseArray(
      raw.callbacks,
      `${path}.callbacks`,
      parseCallback
    );
  }
  if (raw.managed !== undefined) {
    given.managed = parseManaged(raw.managed, `${path}.managed`);
  }
  if (raw.http !== undefined) {
    given.http = parseArray(raw.http, `${path}.http`, parseHttpStub);
  }
  if (raw.engine !== undefined) {
    given.engine = parseEngine(raw.engine, `${path}.engine`);
  }
  if (raw.bindings !== undefined) {
    given.bindings = parseBindings(raw.bindings, `${path}.bindings`);
  }
  return given;
}

function parseExpect(raw: unknown, path: string): Expect {
  if (!isPlainObject(raw)) {
    throw new Error(`rhino-local: ${path} is not an object`);
  }
  rejectUnknownKeys(path, raw, EXPECT_KEYS);
  if (!("outcome" in raw)) {
    throw new Error(
      `rhino-local: ${path}.outcome is required (engine-neutral; use "true" whether the script returns via action.goTo or a legacy outcome variable)`
    );
  }
  if (raw.outcome !== null && typeof raw.outcome !== "string") {
    throw new Error(
      `rhino-local: ${path}.outcome must be a string, or null for a pass that suspends with callbacks`
    );
  }
  const expect: Expect = { outcome: raw.outcome };
  if (raw.sharedState !== undefined) {
    expect.sharedState = parseStateDiff(raw.sharedState, `${path}.sharedState`);
  }
  if (raw.transientState !== undefined) {
    expect.transientState = parseStateDiff(
      raw.transientState,
      `${path}.transientState`
    );
  }
  if (raw.secureState !== undefined) {
    expect.secureState = parseStateDiff(raw.secureState, `${path}.secureState`);
  }
  if (raw.callbacks !== undefined) {
    expect.callbacks = parseArray(
      raw.callbacks,
      `${path}.callbacks`,
      parseCallback
    );
  }
  if (raw.openidm !== undefined) {
    expect.openidm = parseArray(raw.openidm, `${path}.openidm`, parseOpenidmExpect);
  }
  if (raw.http !== undefined) {
    expect.http = parseArray(raw.http, `${path}.http`, parseHttpExpect);
  }
  if (raw.logs !== undefined) {
    expect.logs = parseArray(raw.logs, `${path}.logs`, parseLogExpect);
  }
  if (raw.allowUndeclared !== undefined) {
    expect.allowUndeclared = parseAllowUndeclared(
      raw.allowUndeclared,
      `${path}.allowUndeclared`
    );
  }
  return expect;
}

function parseStateDiff(raw: unknown, path: string): StateDiff {
  if (!isPlainObject(raw)) {
    throw new Error(`rhino-local: ${path} is not an object`);
  }
  rejectUnknownKeys(path, raw, STATE_DIFF_KEYS);
  const diff: StateDiff = {};
  assignJsonObject(diff, "added", raw.added, `${path}.added`);
  assignJsonObject(diff, "changed", raw.changed, `${path}.changed`);
  if (raw.removed !== undefined) {
    if (!Array.isArray(raw.removed) || raw.removed.some((key) => typeof key !== "string")) {
      throw new Error(`rhino-local: ${path}.removed must be an array of strings`);
    }
    diff.removed = raw.removed.slice();
  }
  return diff;
}

function parseHttpStub(raw: unknown, path: string): HttpStub {
  if (!isPlainObject(raw)) {
    throw new Error(`rhino-local: ${path} is not an object`);
  }
  rejectUnknownKeys(path, raw, ["match", "reply"]);
  if (raw.match === undefined) {
    throw new Error(`rhino-local: ${path}.match is required`);
  }
  if (raw.reply === undefined) {
    throw new Error(`rhino-local: ${path}.reply is required`);
  }
  return {
    match: parseHttpMatch(raw.match, `${path}.match`),
    reply: parseHttpReply(raw.reply, `${path}.reply`),
  };
}

function parseHttpMatch(raw: unknown, path: string): HttpMatch {
  if (!isPlainObject(raw)) {
    throw new Error(`rhino-local: ${path} is not an object`);
  }
  rejectUnknownKeys(path, raw, ["url", "method"]);
  if (raw.url === undefined) {
    throw new Error(`rhino-local: ${path}.url is required`);
  }
  const match: HttpMatch = { url: parsePattern(raw.url, `${path}.url`) };
  assignOptionalString(match, "method", raw.method, `${path}.method`);
  return match;
}

function parseHttpReply(raw: unknown, path: string): HttpReply {
  if (!isPlainObject(raw)) {
    throw new Error(`rhino-local: ${path} is not an object`);
  }
  rejectUnknownKeys(path, raw, ["status", "body", "headers"]);
  if (typeof raw.status !== "number" || !Number.isInteger(raw.status)) {
    throw new Error(`rhino-local: ${path}.status must be an integer`);
  }
  const reply: HttpReply = { status: raw.status };
  if (raw.body !== undefined) {
    reply.body = parseJsonValue(raw.body, `${path}.body`);
  }
  assignStringMap(reply, "headers", raw.headers, `${path}.headers`);
  return reply;
}

function parseHttpExpect(raw: unknown, path: string): HttpExpect {
  if (!isPlainObject(raw)) {
    throw new Error(`rhino-local: ${path} is not an object`);
  }
  rejectUnknownKeys(path, raw, ["url", "method", "times"]);
  if (raw.url === undefined) {
    throw new Error(`rhino-local: ${path}.url is required`);
  }
  const expect: HttpExpect = { url: parsePattern(raw.url, `${path}.url`) };
  assignOptionalString(expect, "method", raw.method, `${path}.method`);
  assignTimes(expect, raw.times, `${path}.times`);
  return expect;
}

function parseOpenidmExpect(raw: unknown, path: string): OpenidmExpect {
  if (!isPlainObject(raw)) {
    throw new Error(`rhino-local: ${path} is not an object`);
  }
  rejectUnknownKeys(path, raw, ["method", "resource", "body", "actionName", "times"]);
  if (raw.method === undefined) {
    throw new Error(`rhino-local: ${path}.method is required`);
  }
  if (raw.resource === undefined) {
    throw new Error(`rhino-local: ${path}.resource is required`);
  }
  const method = parseOpenidmMethod(raw.method, `${path}.method`);
  if (raw.actionName !== undefined && method !== "action") {
    throw new Error(
      `rhino-local: ${path}.actionName is only valid when method is "action"`
    );
  }
  const expect: OpenidmExpect = {
    method,
    resource: parsePattern(raw.resource, `${path}.resource`),
  };
  if (raw.body !== undefined) {
    expect.body = parseJsonValue(raw.body, `${path}.body`);
  }
  if (raw.actionName !== undefined) {
    expect.actionName = parsePattern(raw.actionName, `${path}.actionName`);
  }
  assignTimes(expect, raw.times, `${path}.times`);
  return expect;
}

function parseLogExpect(raw: unknown, path: string): LogExpect {
  if (!isPlainObject(raw)) {
    throw new Error(`rhino-local: ${path} is not an object`);
  }
  rejectUnknownKeys(path, raw, ["level", "message", "times"]);
  if (raw.message === undefined) {
    throw new Error(`rhino-local: ${path}.message is required`);
  }
  const expect: LogExpect = {
    message: parsePattern(raw.message, `${path}.message`),
  };
  if (raw.level !== undefined) {
    expect.level = parseLogLevel(raw.level, `${path}.level`);
  }
  assignTimes(expect, raw.times, `${path}.times`);
  return expect;
}

function parseCallback(raw: unknown, path: string): CallbackEffect {
  if (!isPlainObject(raw)) {
    throw new Error(`rhino-local: ${path} is not an object`);
  }
  if (typeof raw.type !== "string" || raw.type.trim() === "") {
    throw new Error(`rhino-local: ${path}.type must be a non-empty string`);
  }
  const callback: CallbackEffect = { type: raw.type };
  for (const [key, value] of Object.entries(raw)) {
    if (key === "type") {
      continue;
    }
    callback[key] = parseJsonValue(value, `${path}.${key}`);
  }
  return callback;
}

function parseAllowUndeclared(raw: unknown, path: string): AllowUndeclared {
  if (!isPlainObject(raw)) {
    throw new Error(`rhino-local: ${path} is not an object`);
  }
  const flags: AllowUndeclared = {};
  for (const key of Object.keys(raw)) {
    if (!ALLOW_UNDECLARED_SET.has(key)) {
      throw unknownKeyError(path, key, ALLOW_UNDECLARED_CHANNELS);
    }
    const value = raw[key];
    if (typeof value !== "boolean") {
      throw new Error(`rhino-local: ${path}.${key} must be a boolean`);
    }
    flags[key as AllowUndeclaredChannel] = value;
  }
  return flags;
}

function parseManaged(
  raw: unknown,
  path: string
): Record<string, JsonObject[]> {
  if (!isPlainObject(raw)) {
    throw new Error(`rhino-local: ${path} is not an object`);
  }
  const managed: Record<string, JsonObject[]> = {};
  for (const [type, records] of Object.entries(raw)) {
    if (!Array.isArray(records)) {
      throw new Error(
        `rhino-local: ${path}.${type} must be an array of managed records`
      );
    }
    managed[type] = records.map((record, index) => {
      const value = parseJsonValue(record, `${path}.${type}[${index}]`);
      if (!isPlainObject(value)) {
        throw new Error(
          `rhino-local: ${path}.${type}[${index}] must be an object`
        );
      }
      return value;
    });
  }
  return managed;
}

function parseBindings(
  raw: unknown,
  path: string
): Record<string, JsonValue> {
  if (!isPlainObject(raw)) {
    throw new Error(`rhino-local: ${path} is not an object`);
  }
  const known = knownBindingNames();
  const bindings: Record<string, JsonValue> = {};
  for (const key of Object.keys(raw)) {
    if (key === "nodeState") {
      throw new Error(
        `rhino-local: ${path}.nodeState is not a seed; use given.sharedState / transientState / secureState`
      );
    }
    if (GIVEN_BINDING_SEED_SET.has(key)) {
      throw new Error(
        `rhino-local: ${path}.${key} collides with given.${key}; seed it there, not under bindings`
      );
    }
    if (!known.has(key)) {
      throw unknownKeyError(
        path,
        key,
        [...known].filter((name) => !GIVEN_BINDING_SEED_SET.has(name) && name !== "nodeState")
      );
    }
    const value = raw[key];
    bindings[key] = parseJsonValue(value, `${path}.${key}`);
  }
  return bindings;
}

function parseEngine(raw: unknown, path: string): Engine {
  if (typeof raw !== "string" || !ENGINE_SET.has(raw)) {
    throw new Error(
      `rhino-local: ${path} must be ${ENGINES.map((engine) => JSON.stringify(engine)).join(" or ")}`
    );
  }
  return raw as Engine;
}

function parseOpenidmMethod(raw: unknown, path: string): OpenidmMethod {
  if (typeof raw !== "string" || !OPENIDM_METHOD_SET.has(raw)) {
    throw new Error(
      `rhino-local: ${path} must be one of ${OPENIDM_METHODS.join(", ")}`
    );
  }
  return raw as OpenidmMethod;
}

function parseLogLevel(raw: unknown, path: string): LogLevel {
  if (typeof raw !== "string" || !LOG_LEVEL_SET.has(raw)) {
    throw new Error(
      `rhino-local: ${path} must be one of ${LOG_LEVELS.join(", ")}`
    );
  }
  return raw as LogLevel;
}

function parsePattern(raw: unknown, path: string): Pattern {
  if (typeof raw === "string") {
    return raw;
  }
  if (raw instanceof RegExp) {
    return raw;
  }
  throw new Error(`rhino-local: ${path} must be a string or RegExp`);
}

function parseNonEmptyString(raw: unknown, path: string): string {
  if (typeof raw !== "string" || raw.trim() === "") {
    throw new Error(`rhino-local: ${path} must be a non-empty string`);
  }
  return raw;
}

function parseArray<T>(
  raw: unknown,
  path: string,
  item: (value: unknown, itemPath: string) => T
): T[] {
  if (!Array.isArray(raw)) {
    throw new Error(`rhino-local: ${path} is not an array`);
  }
  return raw.map((value, index) => item(value, `${path}[${index}]`));
}

function parseStringMap(raw: unknown, path: string): Record<string, string> {
  if (!isPlainObject(raw)) {
    throw new Error(`rhino-local: ${path} is not an object`);
  }
  const map: Record<string, string> = {};
  for (const [key, value] of Object.entries(raw)) {
    if (typeof value !== "string") {
      throw new Error(`rhino-local: ${path}.${key} must be a string`);
    }
    map[key] = value;
  }
  return map;
}

function parseStringArrayMap(
  raw: unknown,
  path: string
): Record<string, string[]> {
  if (!isPlainObject(raw)) {
    throw new Error(`rhino-local: ${path} is not an object`);
  }
  const map: Record<string, string[]> = {};
  for (const [key, value] of Object.entries(raw)) {
    if (
      !Array.isArray(value) ||
      value.some((entry) => typeof entry !== "string")
    ) {
      throw new Error(
        `rhino-local: ${path}.${key} must be an array of strings`
      );
    }
    map[key] = value.slice();
  }
  return map;
}

function assignJsonObject<K extends string>(
  target: { [P in K]?: JsonObject },
  key: K,
  raw: unknown,
  path: string
): void {
  if (raw === undefined) {
    return;
  }
  target[key] = parseJsonObject(raw, path);
}

function assignString<K extends string>(
  target: { [P in K]?: string },
  key: K,
  raw: unknown,
  path: string
): void {
  if (raw === undefined) {
    return;
  }
  if (typeof raw !== "string") {
    throw new Error(`rhino-local: ${path} must be a string`);
  }
  target[key] = raw;
}

function assignOptionalString<K extends string>(
  target: { [P in K]?: string },
  key: K,
  raw: unknown,
  path: string
): void {
  if (raw === undefined) {
    return;
  }
  if (typeof raw !== "string" || raw.trim() === "") {
    throw new Error(`rhino-local: ${path} must be a non-empty string`);
  }
  target[key] = raw;
}

function assignStringMap<K extends string>(
  target: { [P in K]?: Record<string, string> },
  key: K,
  raw: unknown,
  path: string
): void {
  if (raw === undefined) {
    return;
  }
  target[key] = parseStringMap(raw, path);
}

function assignStringArrayMap<K extends string>(
  target: { [P in K]?: Record<string, string[]> },
  key: K,
  raw: unknown,
  path: string
): void {
  if (raw === undefined) {
    return;
  }
  target[key] = parseStringArrayMap(raw, path);
}

function assignTimes(target: { times?: number }, raw: unknown, path: string): void {
  if (raw === undefined) {
    return;
  }
  if (typeof raw !== "number" || !Number.isInteger(raw) || raw < 0) {
    throw new Error(`rhino-local: ${path} must be a non-negative integer`);
  }
  target.times = raw;
}

function rejectUnknownKeys(
  path: string,
  raw: Record<string, unknown>,
  allowed: readonly string[]
): void {
  const allowedSet = new Set(allowed);
  for (const key of Object.keys(raw)) {
    if (!allowedSet.has(key)) {
      throw unknownKeyError(path, key, allowed);
    }
  }
}


