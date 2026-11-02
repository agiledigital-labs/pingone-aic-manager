/** Configuration-wide shape/location checks precede routing and storage work. */
import type { IdmConfig } from "./config.ts";
import { isDocument } from "./document.ts";
import { missing } from "./errors.ts";
import type { Evidence } from "./provenance.ts";

export function validateDeclarations(config: IdmConfig, evidence: Evidence): void {
  const reject = (path: string, expected: string): never => missing(evidence, `schema declaration shape: ${path} (expected ${expected})`);
  const object = (value: unknown, path: string): Record<string, unknown> => {
    if (!isDocument(value)) reject(path, "object");
    return value as Record<string, unknown>;
  };
  const script = (value: unknown, path: string): Record<string, unknown> => {
    if (!isDocument(value)) reject(path, "script object");
    const declaration = value as Record<string, unknown>;
    if (typeof declaration.type !== "string") reject(`${path}/type`, "string");
    for (const key of ["source", "file"]) if (Object.hasOwn(declaration, key) && typeof declaration[key] !== "string") reject(`${path}/${key}`, "string");
    if (Object.hasOwn(declaration, "globals")) object(declaration.globals, `${path}/globals`);
    return declaration;
  };
  const matches = (value: unknown, declaration: Record<string, unknown>): boolean => {
    if (declaration.type === "array") return Array.isArray(value) && (!isDocument(declaration.items) || value.every((item) => matches(item, declaration.items as Record<string, unknown>)));
    if (declaration.type === "object") {
      if (!isDocument(value)) return false;
      const properties = declaration.properties;
      return !isDocument(properties) || Object.entries(value).every(([name, child]) =>
        !Object.hasOwn(properties, name) || (isDocument(properties[name]) && matches(child, properties[name] as Record<string, unknown>)));
    }
    if (declaration.type === "number") return typeof value === "number" && Number.isFinite(value);
    return ["string", "boolean"].includes(String(declaration.type)) && typeof value === declaration.type;
  };
  const visit = (value: unknown, path: string, depth: number): void => {
    const declaration = object(value, path);
    if (Object.hasOwn(declaration, "type") && (typeof declaration.type !== "string" || !["string", "number", "boolean", "object", "array", "relationship"].includes(declaration.type))) missing(evidence, `property behavior type: ${path}`);
    for (const key of ["enum", "required", "policies"]) if (Object.hasOwn(declaration, key)) {
      if (!Array.isArray(declaration[key])) reject(`${path}/${key}`, "array");
      if (key === "required" && !(declaration[key] as unknown[]).every((name) => typeof name === "string")) reject(`${path}/${key}`, "string array");
    }
    for (const hook of ["onValidate", "onStore", "onRetrieve"]) if (Object.hasOwn(declaration, hook)) {
      const hookScript = script(declaration[hook], `${path}/${hook}`);
      if (depth !== 1) {
        if (typeof hookScript.file === "string") missing(evidence, `nested script file reference: ${path}/${hook}: ${hookScript.file}`);
        missing(evidence, `nested property behavior ${hook}: ${path}`);
      }
    }
    if (Object.hasOwn(declaration, "properties")) for (const [name, child] of Object.entries(object(declaration.properties, `${path}/properties`))) visit(child, `${path}/${name}`, depth + 1);
    if (Object.hasOwn(declaration, "items")) visit(object(declaration.items, `${path}/items`), `${path}/items`, depth + 1);
    if (Object.hasOwn(declaration, "default")) {
      if (depth !== 1) missing(evidence, `nested property behavior default: ${path}`);
      if (!matches(declaration.default, declaration)) missing(evidence, `invalid default: ${path}`);
    }
  };
  for (const type of config.managed.objects) {
    visit(type.schema, type.name, 0);
    // Managed roots require properties; otherwise repository code cannot interpret them.
    object(type.schema.properties, `${type.name}/properties`);
    for (const hook of ["onCreate", "onUpdate", "onValidate", "onStore", "postCreate", "postUpdate", "onRetrieve", "onRead", "onDelete", "postDelete", "onSync"]) {
      if (Object.hasOwn(type, hook)) script(type[hook], `${type.name}/${hook}`);
    }
    if (Object.hasOwn(type, "actions")) for (const [name, action] of Object.entries(object(type.actions, `${type.name}/actions`))) script(action, `${type.name}/actions/${name}`);
  }
  for (const [name, endpoint] of Object.entries(config.endpoints)) {
    const declaration = object(endpoint, `endpoint/${name}`);
    script(declaration.type === "scripted" ? declaration.script : declaration, `endpoint/${name}`);
  }
}
