/** Contexts-endpoint binding metadata (`docs/api/bindings/*.json`). */

export const JS_TYPES = [
  "string",
  "number",
  "boolean",
  "object",
  "array",
  "unknown",
  "void",
] as const;

export type JsType = (typeof JS_TYPES)[number];

export interface Parameter {
  name: string;
  javaScriptType: JsType;
}

export interface MethodElement {
  elementType: "method";
  name: string;
  parameters: Parameter[];
  returnType: JsType;
}

export interface FieldElement {
  elementType: "field";
  name: string;
  javaScriptType: JsType;
  elements: Element[];
}

export type Element = MethodElement | FieldElement;

export interface Binding {
  name: string;
  javaScriptType: JsType;
  elements: Element[];
}

export interface ContextsDocument {
  _id: string;
  bindings: Binding[];
}

export type BindingKind = "scalar" | "opaque" | "object";

export function classifyBinding(binding: Binding): BindingKind {
  if (
    binding.javaScriptType === "string" ||
    binding.javaScriptType === "number" ||
    binding.javaScriptType === "boolean"
  ) {
    if (binding.elements.length > 0) {
      throw new Error(
        `rhino-local: scalar binding "${binding.name}" unexpectedly has elements`
      );
    }
    return "scalar";
  }
  if (binding.elements.length === 0) {
    return "opaque";
  }
  return "object";
}

export interface MethodMember {
  kind: "method";
  name: string;
  overloads: MethodElement[];
}

export interface FieldMember {
  kind: "field";
  name: string;
  javaScriptType: JsType;
  elements: Element[];
}

export type Member = MethodMember | FieldMember;

/** Group overloads of the same name; preserve first-seen member order. */
export function membersOf(elements: Element[]): Member[] {
  const order: string[] = [];
  const methods = new Map<string, MethodElement[]>();
  const fields = new Map<string, FieldElement>();

  for (const element of elements) {
    if (element.elementType === "method") {
      if (fields.has(element.name)) {
        throw new Error(
          `rhino-local: "${element.name}" is both a method and a field`
        );
      }
      let overloads = methods.get(element.name);
      if (!overloads) {
        order.push(element.name);
        overloads = [];
        methods.set(element.name, overloads);
      }
      overloads.push(element);
      continue;
    }
    if (methods.has(element.name)) {
      throw new Error(
        `rhino-local: "${element.name}" is both a method and a field`
      );
    }
    if (!fields.has(element.name)) {
      order.push(element.name);
    }
    fields.set(element.name, element);
  }

  return order.map((name) => {
    const overloads = methods.get(name);
    if (overloads) {
      return { kind: "method", name, overloads: uniqueOverloads(overloads) };
    }
    const field = fields.get(name);
    if (!field) {
      throw new Error(`rhino-local: missing grouped member "${name}"`);
    }
    return {
      kind: "field",
      name,
      javaScriptType: field.javaScriptType,
      elements: field.elements,
    };
  });
}

export function uniqueOverloads(overloads: MethodElement[]): MethodElement[] {
  const seen = new Set<string>();
  const unique: MethodElement[] = [];
  for (const overload of overloads) {
    // Collapse identical JSON copies (callbacksBuilder.confirmationCallback
    // repeats the same signature) but keep overloads that differ only by
    // parameter name (logger.info(format, arg) vs info(msg, t)).
    const key = `${overloadLabel(overload)}:${overload.returnType}`;
    if (seen.has(key)) {
      continue;
    }
    seen.add(key);
    unique.push(overload);
  }
  return unique;
}

export function overloadKey(overload: MethodElement): string {
  return `${overload.parameters.map((parameter) => parameter.javaScriptType).join(",")}->${overload.returnType}`;
}

export function overloadLabel(overload: MethodElement): string {
  const params = overload.parameters
    .map((parameter) => `${parameter.name}: ${parameter.javaScriptType}`)
    .join(", ");
  return `${overload.name}(${params})`;
}
