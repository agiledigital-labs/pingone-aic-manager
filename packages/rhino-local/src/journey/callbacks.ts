/** Callback wire envelopes consumed by aic/callbacks.ts; AM binding effects stay shared. */
import type { CallbackEffect } from "../case/types.ts";
import type { JsonValue } from "../protocol.ts";
import { isDocument } from "../idm/document.ts";
export type WireCallback = { type: string; output: { name: string; value: JsonValue }[]; input?: { name: string; value: JsonValue }[] };
const SUPPORTED = ["NameCallback", "PasswordCallback", "TextInputCallback", "TextOutputCallback", "HiddenValueCallback"];
export function toWire(callbacks: CallbackEffect[], absent: (name: string) => never): WireCallback[] {
  return callbacks.map((callback, index) => {
    if (!SUPPORTED.includes(callback.type)) absent(`callback wire type: ${callback.type}`);
    const output = Object.entries(callback).filter(([key]) => key !== "type").map(([name, value]) => ({ name, value }));
    const out: WireCallback = { type: callback.type, output };
    if (callback.type !== "TextOutputCallback") out.input = [{ name: `IDToken${index + 1}`, value: callback.type === "HiddenValueCallback" ? callback.id ?? "" : callback.type === "TextInputCallback" ? callback.defaultText ?? "" : "" }];
    return out;
  });
}
export function fromWire(raw: JsonValue | undefined, absent: (name: string) => never): CallbackEffect[] {
  if (raw === undefined) return [];
  if (!Array.isArray(raw)) absent("authenticate callbacks shape");
  return raw.map((callback) => {
    if (!isDocument(callback) || typeof callback.type !== "string") absent("authenticate callback shape");
    if (!SUPPORTED.includes(callback.type)) absent(`callback wire type: ${callback.type}`);
    if (callback.type !== "TextOutputCallback" && (!Array.isArray(callback.input) || !isDocument(callback.input[0]) || typeof callback.input[0].value !== "string")) absent("authenticate callback input shape");
    const effect: CallbackEffect = { type: callback.type };
    if (Array.isArray(callback.output)) for (const field of callback.output) if (isDocument(field) && typeof field.name === "string" && field.value !== undefined) effect[field.name] = field.value;
    if (Array.isArray(callback.input) && isDocument(callback.input[0]) && callback.input[0].value !== undefined) effect.value = callback.input[0].value;
    return effect;
  });
}
