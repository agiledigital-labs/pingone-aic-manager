import type { JsonObject, JsonValue } from "./types.ts";

export function isPlainObject(value: unknown): value is Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    return false;
  }
  const proto = Object.getPrototypeOf(value);
  return proto === Object.prototype || proto === null;
}

export function formatValue(value: unknown): string {
  if (value === undefined) {
    return "<absent>";
  }
  if (value instanceof RegExp) {
    return String(value);
  }
  if (typeof value === "string") {
    return JSON.stringify(value);
  }
  if (typeof value === "function") {
    return "<function>";
  }
  try {
    return JSON.stringify(value);
  } catch {
    return String(value);
  }
}

export function suggestKey(
  unknown: string,
  allowed: readonly string[]
): string | undefined {
  const lower = unknown.toLowerCase();
  const contained = allowed.filter(
    (key) =>
      key.toLowerCase().includes(lower) || lower.includes(key.toLowerCase())
  );
  if (contained.length === 1) {
    return contained[0];
  }

  let best: string | undefined;
  let bestDistance = Infinity;
  for (const key of allowed) {
    const distance = editDistance(lower, key.toLowerCase());
    if (distance < bestDistance) {
      bestDistance = distance;
      best = key;
    }
  }
  if (best !== undefined && bestDistance <= 2) {
    return best;
  }
  return contained[0];
}

export function unknownKeyError(
  path: string,
  key: string,
  allowed: readonly string[]
): Error {
  const suggestion = suggestKey(key, allowed);
  const hint = suggestion === undefined ? "" : ` (did you mean "${suggestion}"?)`;
  return new Error(
    `rhino-local: ${path} has unknown key "${key}"${hint}. Allowed: ${allowed.join(", ")}`
  );
}

function editDistance(a: string, b: string): number {
  const rows = a.length;
  const cols = b.length;
  const width = cols + 1;
  const dp = new Int32Array((rows + 1) * width);
  for (let i = 0; i <= rows; i += 1) {
    dp[i * width] = i;
  }
  for (let j = 0; j <= cols; j += 1) {
    dp[j] = j;
  }
  for (let i = 1; i <= rows; i += 1) {
    for (let j = 1; j <= cols; j += 1) {
      const cost = a.charAt(i - 1) === b.charAt(j - 1) ? 0 : 1;
      const deletion = (dp[(i - 1) * width + j] ?? 0) + 1;
      const insertion = (dp[i * width + (j - 1)] ?? 0) + 1;
      const substitution = (dp[(i - 1) * width + (j - 1)] ?? 0) + cost;
      dp[i * width + j] = Math.min(deletion, insertion, substitution);
    }
  }
  return dp[rows * width + cols] ?? Math.max(rows, cols);
}

export function parseJsonValue(raw: unknown, path: string): JsonValue {
  if (raw === null || typeof raw === "string" || typeof raw === "boolean") {
    return raw;
  }
  if (typeof raw === "number") {
    if (!Number.isFinite(raw)) {
      throw new Error(`rhino-local: ${path} must be a finite number`);
    }
    return raw;
  }
  if (Array.isArray(raw)) {
    return raw.map((item, index) => parseJsonValue(item, `${path}[${index}]`));
  }
  if (isPlainObject(raw)) {
    const object: JsonObject = {};
    for (const [key, value] of Object.entries(raw)) {
      object[key] = parseJsonValue(value, `${path}.${key}`);
    }
    return object;
  }
  throw new Error(
    `rhino-local: ${path} is not a JSON value (${describeType(raw)})`
  );
}

export function parseJsonObject(raw: unknown, path: string): JsonObject {
  const value = parseJsonValue(raw, path);
  if (!isPlainObject(value)) {
    throw new Error(`rhino-local: ${path} must be an object`);
  }
  return value;
}

function describeType(value: unknown): string {
  if (value === null) {
    return "null";
  }
  if (Array.isArray(value)) {
    return "array";
  }
  if (value instanceof RegExp) {
    return "RegExp";
  }
  return typeof value;
}
