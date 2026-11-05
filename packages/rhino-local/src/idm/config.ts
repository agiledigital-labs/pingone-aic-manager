/** Immutable tenant config snapshot, shared by emulator forks. No disk loading. */
import type { JsonValue } from "../protocol.ts";
import type { ConfigInput, EndpointConfig, ManagedConfig, ManagedType } from "./types.ts";

/**
 * Where DS stores a managed type's records. `generic`: JSON in `ou=managed` (the `managed/*`
 * generic mapping, or a per-type `genericMapping` entry); `explicit`: a type with its own LDAP
 * object class; `unknown`: no `repo` supplied, or a repo naming the type in both sections.
 * `alpha_user` / `bravo_user` are explicit by name (docs/api/14).
 */
export type StorageKind = "generic" | "explicit" | "unknown";

const USER_TYPES = ["alpha_user", "bravo_user"];

function section(repo: Readonly<Record<string, JsonValue>> | undefined, name: string): Record<string, JsonValue> | undefined {
  const mapping = repo?.resourceMapping;
  if (mapping === null || typeof mapping !== "object" || Array.isArray(mapping)) return undefined;
  const value = (mapping as Record<string, JsonValue>)[name];
  return value !== null && typeof value === "object" && !Array.isArray(value) ? value as Record<string, JsonValue> : undefined;
}
const has = (object: Record<string, JsonValue> | undefined, key: string): boolean =>
  object !== undefined && Object.prototype.hasOwnProperty.call(object, key);

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
  private readonly repo: Readonly<Record<string, JsonValue>> | undefined;
  /** Mapping names by source resource (`managed/<type>`), or undefined without a `sync` input. */
  private readonly syncSources: ReadonlyMap<string, readonly string[]> | undefined;

  constructor(input: ConfigInput) {
    plainObject(input, "input");
    plainObject(input.managed, "managed");
    if (!Array.isArray(input.managed.objects)) throw new TypeError("Invalid IDM configuration: managed.objects must be an array");
    for (const [index, type] of input.managed.objects.entries()) {
      plainObject(type, `managed.objects[${index}]`);
      if (typeof type.name !== "string" || type.name.length === 0) throw new TypeError(`Invalid IDM configuration: managed.objects[${index}].name must be a nonempty string`);
    }
    if (input.endpoints !== undefined) plainObject(input.endpoints, "endpoints");
    if (input.repo !== undefined) plainObject(input.repo, "repo");
    if (input.sync !== undefined) {
      plainObject(input.sync, "sync");
      if (!Array.isArray(input.sync.mappings)) throw new TypeError("Invalid IDM configuration: sync.mappings must be an array");
      for (const [index, mapping] of input.sync.mappings.entries()) {
        plainObject(mapping, `sync.mappings[${index}]`);
        const { name, source } = mapping as Record<string, JsonValue>;
        if (typeof name !== "string" || name.length === 0 || typeof source !== "string" || source.length === 0) {
          throw new TypeError(`Invalid IDM configuration: sync.mappings[${index}] needs a nonempty string name and source`);
        }
      }
    }
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
    this.repo = snapshot.repo;
    if (snapshot.sync === undefined) this.syncSources = undefined;
    else {
      const sources = new Map<string, string[]>();
      for (const mapping of snapshot.sync.mappings as Array<Record<string, JsonValue>>) {
        const source = (mapping.source as string).replace(/^\/+|\/+$/g, "");
        sources.set(source, [...(sources.get(source) ?? []), mapping.name as string]);
      }
      this.syncSources = sources;
    }
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

  /**
   * The sync mappings whose source is `managed/<type>`, in config order: empty when the supplied
   * `config/sync` has none, undefined when no `sync` was supplied.
   */
  syncMappings(type: string): readonly string[] | undefined {
    return this.syncSources === undefined ? undefined : this.syncSources.get(`managed/${type}`) ?? [];
  }

  /** The type's storage kind, read from the supplied `config/repo.ds` (see `StorageKind`). */
  storage(type: string): StorageKind {
    if (USER_TYPES.includes(type)) return "explicit";
    if (this.repo === undefined) return "unknown";
    const key = `managed/${type}`;
    const explicit = has(section(this.repo, "explicitMapping"), key);
    const generic = section(this.repo, "genericMapping");
    const own = has(generic, key);
    if (explicit && own) return "unknown";
    if (explicit) return "explicit";
    return own || has(generic, "managed/*") ? "generic" : "unknown";
  }

  /**
   * Whether a top-level property of a type is stored as JSON in DS: a generic type's property
   * that its `genericMapping` entry does not map to an attribute of its own (a mapped
   * relationship is listed under `properties`).
   */
  jsonStored(type: string, field: string): boolean {
    if (field.includes("/") || this.storage(type) !== "generic") return false;
    const entry = section(this.repo, "genericMapping")?.[`managed/${type}`];
    const properties = entry !== null && typeof entry === "object" && !Array.isArray(entry) ? (entry as Record<string, JsonValue>).properties : undefined;
    return !(properties !== null && typeof properties === "object" && !Array.isArray(properties) && has(properties as Record<string, JsonValue>, field));
  }
}
