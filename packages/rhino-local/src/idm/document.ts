/** JSON pointer projection and CREST patch operations, shared by REST and host calls. */
import type { JsonValue } from "../protocol.ts";
import { invalidInput, missing } from "./errors.ts";
import { fieldValue, pointerParts } from "./filter.ts";
import type { Evidence } from "./provenance.ts";
import type { Document } from "./types.ts";

export function isDocument(value: unknown): value is Document {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

export function document(value: JsonValue | undefined): Document {
  if (!isDocument(value)) throw invalidInput("Expecting JSON object content");
  return structuredClone(value);
}

export function fields(value: JsonValue | undefined): string[] {
  if (value === undefined) return [];
  if (typeof value === "string") return value.split(",");
  if (Array.isArray(value) && value.every((entry) => typeof entry === "string")) {
    return value.flatMap((entry) => (entry as string).split(","));
  }
  throw invalidInput("Invalid _fields parameter");
}

export function project(object: Document, selectors: string[], evidence: Evidence): Document {
  if (selectors.length === 0 || selectors.includes("*")) return structuredClone(object);
  evidence.measured("field projection preserves _id/_rev and materializes selected nulls", "docs/api/10-managed-objects.md");
  const result: Document = {};
  if (object._id !== undefined) result._id = object._id;
  if (object._rev !== undefined) result._rev = object._rev;
  for (const selector of selectors) {
    if (selector.startsWith(" ")) continue;
    const parts = pointerParts(selector);
    if (parts.includes("*")) missing(evidence, "nested wildcard field projection");
    if (parts.length > 1) evidence.assumed("nested field projection on ordinary JSON objects");
    let target = result;
    for (let i = 0; i < parts.length; i++) {
      const key = parts[i];
      if (key === undefined) continue;
      if (i === parts.length - 1) target[key] = structuredClone(fieldValue(object, selector) ?? null);
      else {
        if (!isDocument(target[key])) target[key] = {};
        target = target[key] as Document;
      }
    }
  }
  return result;
}

export function applyPatch(object: Document, operations: JsonValue | undefined, evidence: Evidence): Document {
  if (!Array.isArray(operations)) throw invalidInput("Expecting a list of patch operations");
  const result = structuredClone(object);
  for (const raw of operations) {
    const op = document(raw);
    if (!["add", "replace", "remove"].includes(String(op.operation))) {
      missing(evidence, `patch operation ${String(op.operation)}`);
    }
    if (typeof op.field !== "string" || !op.field.startsWith("/") || op.field === "/") {
      throw invalidInput("Patch field must be a non-empty JSON pointer");
    }
    const parts = pointerParts(op.field);
    if (parts[0] === "_id" || parts[0] === "_rev") missing(evidence, "patching managed identity fields");
    let parent: Document | JsonValue[] = result;
    for (const part of parts.slice(0, -1)) {
      const next: JsonValue | undefined = (parent as Document)[part];
      if (!isDocument(next) && !Array.isArray(next)) throw invalidInput(`Patch parent does not exist: ${op.field}`);
      parent = next;
    }
    const key = parts.at(-1);
    if (key === undefined) throw invalidInput("Invalid patch field");
    evidence.assumed("patch add/replace/remove use JSON pointer mutation semantics");
    if (Array.isArray(parent)) {
      const index = key === "-" && op.operation === "add" ? parent.length : Number(key);
      const max = op.operation === "add" ? parent.length : parent.length - 1;
      if (!Number.isInteger(index) || index < 0 || index > max) throw invalidInput("Patch array index out of bounds");
      if (op.operation === "remove") parent.splice(index, 1);
      else if (op.value === undefined) throw invalidInput("Patch value is required");
      else if (op.operation === "add") parent.splice(index, 0, structuredClone(op.value));
      else parent[index] = structuredClone(op.value);
    } else if (op.operation === "remove") {
      delete parent[key];
    } else {
      if (op.value === undefined) throw invalidInput("Patch value is required");
      Object.defineProperty(parent, key, { value: structuredClone(op.value), writable: true, enumerable: true, configurable: true });
    }
  }
  return result;
}
