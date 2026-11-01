/** Immutable tenant config snapshot, shared by emulator forks. No disk loading. */
import type { ConfigInput, EndpointConfig, ManagedConfig, ManagedType } from "./types.ts";

function freeze(value: unknown): void {
  if (value === null || typeof value !== "object" || Object.isFrozen(value)) return;
  if (!Array.isArray(value)) Object.setPrototypeOf(value, null);
  for (const child of Object.values(value)) freeze(child);
  Object.freeze(value);
}

export class IdmConfig {
  readonly managed: ManagedConfig;
  readonly endpoints: Readonly<Record<string, EndpointConfig>>;
  private readonly types: ReadonlyMap<string, ManagedType>;

  constructor(input: ConfigInput) {
    const snapshot = structuredClone(input);
    freeze(snapshot);
    this.managed = snapshot.managed;
    this.endpoints = snapshot.endpoints ?? Object.freeze({});
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
