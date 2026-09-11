import { CHANNELS, LOG_LEVELS, OPENIDM_METHODS } from "../case/types.ts";
import type {
  CallbackEffect,
  HttpEffect,
  LogEffect,
  LogLevel,
  OpenidmEffect,
  OpenidmMethod,
  RecordedEffects,
  StateBucket,
} from "../case/types.ts";
import { isPlainObject, parseJsonObject, parseJsonValue } from "../case/util.ts";

const LOG_LEVEL_SET: ReadonlySet<string> = new Set(LOG_LEVELS);
const OPENIDM_METHOD_SET: ReadonlySet<string> = new Set(OPENIDM_METHODS);

/**
 * Parse the JSON string `__rhinoLocalHarvest()` returns. The JVM runner can
 * only carry primitives as completion values (`JsValues` stringifies objects),
 * so harvest is a string on purpose.
 */
export function parseHarvest(raw: string): RecordedEffects {
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw) as unknown;
  } catch (error) {
    throw new Error(
      `rhino-local: harvest is not JSON: ${error instanceof Error ? error.message : String(error)}`
    );
  }
  if (!isPlainObject(parsed)) {
    throw new Error("rhino-local: harvest is not an object");
  }
  for (const key of CHANNELS) {
    if (!(key in parsed)) {
      throw new Error(
        `rhino-local: harvest is missing ${key} — a runner that does not record a channel would silently assert nothing`
      );
    }
  }
  if (parsed.outcome !== null && typeof parsed.outcome !== "string") {
    throw new Error("rhino-local: harvest.outcome must be a string or null");
  }
  return {
    outcome: parsed.outcome,
    sharedState: parseBucket(parsed.sharedState, "harvest.sharedState"),
    transientState: parseBucket(parsed.transientState, "harvest.transientState"),
    secureState: parseBucket(parsed.secureState, "harvest.secureState"),
    callbacks: parseArray(parsed.callbacks, "harvest.callbacks", parseCallback),
    openidm: parseArray(parsed.openidm, "harvest.openidm", parseOpenidm),
    http: parseArray(parsed.http, "harvest.http", parseHttp),
    logs: parseArray(parsed.logs, "harvest.logs", parseLog),
  };
}

function parseBucket(raw: unknown, path: string): StateBucket {
  if (!isPlainObject(raw)) {
    throw new Error(`rhino-local: ${path} is not an object`);
  }
  return {
    initial: parseJsonObject(raw.initial, `${path}.initial`),
    final: parseJsonObject(raw.final, `${path}.final`),
  };
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

function parseOpenidm(raw: unknown, path: string): OpenidmEffect {
  if (!isPlainObject(raw)) {
    throw new Error(`rhino-local: ${path} is not an object`);
  }
  if (typeof raw.method !== "string" || !OPENIDM_METHOD_SET.has(raw.method)) {
    throw new Error(
      `rhino-local: ${path}.method must be one of ${OPENIDM_METHODS.join(", ")}`
    );
  }
  if (typeof raw.resource !== "string") {
    throw new Error(`rhino-local: ${path}.resource must be a string`);
  }
  const effect: OpenidmEffect = {
    method: raw.method as OpenidmMethod,
    resource: raw.resource,
  };
  if (raw.body !== undefined) {
    effect.body = parseJsonValue(raw.body, `${path}.body`);
  }
  if (raw.actionName !== undefined) {
    if (typeof raw.actionName !== "string") {
      throw new Error(`rhino-local: ${path}.actionName must be a string`);
    }
    effect.actionName = raw.actionName;
  }
  return effect;
}

function parseHttp(raw: unknown, path: string): HttpEffect {
  if (!isPlainObject(raw)) {
    throw new Error(`rhino-local: ${path} is not an object`);
  }
  if (typeof raw.url !== "string") {
    throw new Error(`rhino-local: ${path}.url must be a string`);
  }
  if (typeof raw.method !== "string" || raw.method.trim() === "") {
    throw new Error(`rhino-local: ${path}.method must be a non-empty string`);
  }
  const effect: HttpEffect = { url: raw.url, method: raw.method };
  if (raw.body !== undefined) {
    effect.body = parseJsonValue(raw.body, `${path}.body`);
  }
  return effect;
}

function parseLog(raw: unknown, path: string): LogEffect {
  if (!isPlainObject(raw)) {
    throw new Error(`rhino-local: ${path} is not an object`);
  }
  if (typeof raw.level !== "string" || !LOG_LEVEL_SET.has(raw.level)) {
    throw new Error(
      `rhino-local: ${path}.level must be one of ${LOG_LEVELS.join(", ")}`
    );
  }
  if (typeof raw.message !== "string") {
    throw new Error(`rhino-local: ${path}.message must be a string`);
  }
  return { level: raw.level as LogLevel, message: raw.message };
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
