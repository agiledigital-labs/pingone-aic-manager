import type { CallbackEffect, JsonValue } from "../case/types.ts";
import { isPlainObject, parseJsonValue } from "../case/util.ts";
import { HARNESS_CALLBACK_ID } from "./constants.ts";

export interface AuthenticateCallbacks {
  /** JSON payload from the result node's HiddenValueCallback, if present. */
  dumpRaw: string | undefined;
  /** Subject-node callbacks; harness dump stripped. */
  callbacks: CallbackEffect[];
}

/**
 * Read `/authenticate` callbacks. `type` is taken from the HTTP field as-is
 * (the effects-contract spelling: Java simple class name, PascalCase).
 * Other fields are the callback's `output` name/value pairs, so a NameCallback
 * becomes `{ type: "NameCallback", prompt: "User Name" }` — no `input` (that
 * is the client's reply, not the effect).
 */
export function parseAuthenticateCallbacks(body: unknown): AuthenticateCallbacks {
  if (!isPlainObject(body)) {
    throw new Error("rhino-local: authenticate body is not an object");
  }
  const rawCallbacks = body.callbacks;
  if (rawCallbacks === undefined) {
    return { dumpRaw: undefined, callbacks: [] };
  }
  if (!Array.isArray(rawCallbacks)) {
    throw new Error("rhino-local: authenticate.callbacks is not an array");
  }
  let dumpRaw: string | undefined;
  const callbacks: CallbackEffect[] = [];
  for (let index = 0; index < rawCallbacks.length; index += 1) {
    const parsed = parseOneCallback(rawCallbacks[index], `callbacks[${index}]`);
    if (parsed.harnessValue !== undefined) {
      dumpRaw = parsed.harnessValue;
      continue;
    }
    callbacks.push(parsed.effect);
  }
  return { dumpRaw, callbacks };
}

function parseOneCallback(
  raw: unknown,
  path: string
): { effect: CallbackEffect; harnessValue?: string } {
  if (!isPlainObject(raw)) {
    throw new Error(`rhino-local: ${path} is not an object`);
  }
  if (typeof raw.type !== "string" || raw.type.trim() === "") {
    throw new Error(`rhino-local: ${path}.type must be a non-empty string`);
  }
  const effect: CallbackEffect = { type: raw.type };
  const output = raw.output;
  let id: string | undefined;
  let value: JsonValue | undefined;
  if (output !== undefined) {
    if (!Array.isArray(output)) {
      throw new Error(`rhino-local: ${path}.output is not an array`);
    }
    for (let index = 0; index < output.length; index += 1) {
      const item = output[index];
      if (!isPlainObject(item) || typeof item.name !== "string") {
        continue;
      }
      if (item.name === "_id") {
        continue;
      }
      const parsed = parseJsonValue(item.value ?? null, `${path}.output[${index}].value`);
      if (item.name === "id") {
        id = typeof parsed === "string" ? parsed : String(parsed);
        effect.id = id;
        continue;
      }
      if (item.name === "value") {
        value = parsed;
      }
      effect[item.name] = parsed;
    }
  }
  if (raw.type === "HiddenValueCallback" && id === HARNESS_CALLBACK_ID) {
    if (typeof value !== "string") {
      throw new Error(
        `rhino-local: harness HiddenValueCallback value must be a string, got ${typeof value}`
      );
    }
    return { effect, harnessValue: value };
  }
  return { effect };
}
