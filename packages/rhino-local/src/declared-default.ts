/**
 * Whether a schema `default` matches the property it is declared on. One rule for both lanes: the
 * IDM emulator's config check (`idm/declarations.ts`) and the script tester's profile table
 * (`profile/write-guard.ts`) call this, so they cannot disagree about which declaration is invalid.
 *
 * Measured (docs/api/10-managed-objects.md): `{"type": "boolean", "default": "nope"}` is accepted by
 * the config `PUT`, and the managed object then answers 404 to every call. Every other mismatch,
 * a `null` default included, is judged by the same rule without a measurement of its own; callers
 * fault it rather than serve the type.
 *
 * Only the four JSON scalar/container types IDM declares are accepted, and `type` must be a single
 * string: a type list (`["string", "null"]`), `relationship`, or no type at all never matches.
 * An `object` default is checked against its declared `properties` where the declaration has them.
 */
export interface DefaultDeclaration {
  readonly type?: unknown;
  readonly items?: unknown;
  readonly properties?: unknown;
}

function isDocument(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

export function defaultMatches(value: unknown, declaration: DefaultDeclaration): boolean {
  if (declaration.type === "array") {
    const items = declaration.items;
    return Array.isArray(value) && (!isDocument(items) || value.every((item) => defaultMatches(item, items)));
  }
  if (declaration.type === "object") {
    if (!isDocument(value)) return false;
    const properties = declaration.properties;
    return !isDocument(properties) || Object.entries(value).every(([name, child]) =>
      !Object.hasOwn(properties, name) || (isDocument(properties[name]) && defaultMatches(child, properties[name])));
  }
  if (declaration.type === "number") return typeof value === "number" && Number.isFinite(value);
  return ["string", "boolean"].includes(String(declaration.type)) && typeof value === declaration.type;
}
