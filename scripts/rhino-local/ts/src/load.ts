import { readFileSync } from "node:fs";
import { JS_TYPES } from "./schema.ts";
import type { Binding, ContextsDocument, Element, JsType, Parameter } from "./schema.ts";

const JS_TYPE_SET: Set<string> = new Set(JS_TYPES);

export function loadContext(path: string): ContextsDocument {
  let parsed: unknown;
  try {
    parsed = JSON.parse(readFileSync(path, "utf8"));
  } catch (error) {
    throw new Error(`rhino-local: cannot parse ${path}: ${String(error)}`);
  }
  return parseDocument(parsed, path);
}

function parseDocument(raw: unknown, path: string): ContextsDocument {
  if (!isRecord(raw)) {
    throw new Error(`rhino-local: ${path} is not an object`);
  }
  if (typeof raw._id !== "string" || raw._id.length === 0) {
    throw new Error(`rhino-local: ${path} is missing _id`);
  }
  if (!Array.isArray(raw.bindings)) {
    throw new Error(`rhino-local: ${path} is missing bindings[]`);
  }
  return {
    _id: raw._id,
    bindings: raw.bindings.map((binding, index) =>
      parseBinding(binding, `${path} bindings[${index}]`)
    ),
  };
}

function parseBinding(raw: unknown, path: string): Binding {
  if (!isRecord(raw)) {
    throw new Error(`rhino-local: ${path} is not an object`);
  }
  if (typeof raw.name !== "string") {
    throw new Error(`rhino-local: ${path} is missing name`);
  }
  return {
    name: raw.name,
    javaScriptType: parseJsType(raw.javaScriptType, `${path}.javaScriptType`),
    elements: parseElements(raw.elements, `${path}.elements`),
  };
}

function parseElements(raw: unknown, path: string): Element[] {
  if (raw === undefined || raw === null) {
    return [];
  }
  if (!Array.isArray(raw)) {
    throw new Error(`rhino-local: ${path} is not an array`);
  }
  return raw.map((element, index) => parseElement(element, `${path}[${index}]`));
}

function parseElement(raw: unknown, path: string): Element {
  if (!isRecord(raw)) {
    throw new Error(`rhino-local: ${path} is not an object`);
  }
  if (typeof raw.name !== "string") {
    throw new Error(`rhino-local: ${path} is missing name`);
  }
  if (raw.elementType === "method") {
    return {
      elementType: "method",
      name: raw.name,
      parameters: parseParameters(raw.parameters, `${path}.parameters`),
      returnType: parseJsType(raw.returnType, `${path}.returnType`),
    };
  }
  if (raw.elementType === "field") {
    return {
      elementType: "field",
      name: raw.name,
      javaScriptType: parseJsType(raw.javaScriptType, `${path}.javaScriptType`),
      elements: parseElements(raw.elements, `${path}.elements`),
    };
  }
  throw new Error(
    `rhino-local: ${path} has unknown elementType ${JSON.stringify(raw.elementType)}`
  );
}

function parseParameters(raw: unknown, path: string): Parameter[] {
  if (raw === undefined || raw === null) {
    return [];
  }
  if (!Array.isArray(raw)) {
    throw new Error(`rhino-local: ${path} is not an array`);
  }
  return raw.map((parameter, index) => {
    if (!isRecord(parameter)) {
      throw new Error(`rhino-local: ${path}[${index}] is not an object`);
    }
    if (typeof parameter.name !== "string") {
      throw new Error(`rhino-local: ${path}[${index}] is missing name`);
    }
    return {
      name: parameter.name,
      javaScriptType: parseJsType(
        parameter.javaScriptType,
        `${path}[${index}].javaScriptType`
      ),
    };
  });
}

function parseJsType(raw: unknown, path: string): JsType {
  if (typeof raw !== "string" || !JS_TYPE_SET.has(raw)) {
    throw new Error(`rhino-local: ${path} unexpected javaScriptType ${JSON.stringify(raw)}`);
  }
  return raw as JsType;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
