/** Immutable tenant config snapshot, shared by emulator forks. No disk loading. */
import type { ConfigInput, EndpointConfig, ManagedConfig, ManagedType } from "./types.ts";

/** Validate caller data before cloning can erase a non-plain prototype. */
function plainObject(value: unknown, path: string): void {
  if (value === null || typeof value !== "object" || Array.isArray(value) ||
      (Object.getPrototypeOf(value) !== Object.prototype && Object.getPrototypeOf(value) !== null)) {
    throw new TypeError(`Invalid IDM configuration: ${path} must be a plain object`);
  }
}

function freeze(value: unknown): void {
  if (value === null || typeof value !== "object" || Object.isFrozen(value)) return;
  if (!Array.isArray(value)) Object.setPrototypeOf(value, null);
  for (const child of Object.values(value)) freeze(child);
  Object.freeze(value);
}

export class IdmConfig {
  readonly managed: ManagedConfig;
  readonly endpoints: Readonly<Record<string, EndpointConfig>>;
  /** Presence of a snapshot alone does not establish namespace completeness. */
  readonly endpointsSupplied: boolean;
  readonly complete: Readonly<{ managed: boolean; endpoints: boolean }>;
  private readonly types: ReadonlyMap<string, ManagedType>;

  constructor(input: ConfigInput) {
    plainObject(input, "input");
    plainObject(input.managed, "managed");
    if (!Array.isArray(input.managed.objects)) throw new TypeError("Invalid IDM configuration: managed.objects must be an array");
    for (const [index, type] of input.managed.objects.entries()) {
      plainObject(type, `managed.objects[${index}]`);
      if (typeof type.name !== "string" || type.name.length === 0) throw new TypeError(`Invalid IDM configuration: managed.objects[${index}].name must be a nonempty string`);
    }
    if (input.endpoints !== undefined) plainObject(input.endpoints, "endpoints");
    if (input.complete !== undefined) {
      plainObject(input.complete, "complete");
      for (const [namespace, value] of Object.entries(input.complete)) {
        if (!["managed", "endpoints"].includes(namespace) || typeof value !== "boolean") {
          throw new TypeError("Invalid IDM configuration: complete supports only boolean managed/endpoints flags");
        }
      }
      if (input.complete.endpoints === true && input.endpoints === undefined) {
        throw new TypeError("Invalid IDM configuration: complete.endpoints requires an endpoints map");
      }
    }
    const snapshot = structuredClone(input);
    freeze(snapshot);
    this.managed = snapshot.managed;
    this.endpointsSupplied = snapshot.endpoints !== undefined;
    this.endpoints = snapshot.endpoints ?? Object.freeze({});
    this.complete = Object.freeze({ managed: snapshot.complete?.managed === true, endpoints: snapshot.complete?.endpoints === true });
    const types = new Map<string, ManagedType>();
    for (const type of this.managed.objects) {
      if (types.has(type.name)) throw new Error(`Duplicate managed type: ${type.name}`);
      types.set(type.name, type);
    }
    this.types = types;
    Object.freeze(this);
  }

  managedType(name: string): ManagedType | undefined {
    return this.types.get(name);
  }
}
