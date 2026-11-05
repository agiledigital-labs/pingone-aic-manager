/** REST and config contracts for the IDM core. No test framework or transport types. */
import type { EmulationFailureInfo, JsonValue } from "../protocol.ts";

export type Document = { [key: string]: JsonValue };
export type Script = {
  readonly type: string;
  readonly source?: string;
  readonly file?: string;
  readonly globals?: Readonly<Document>;
};
export type PropertyConfig = {
  readonly type?: string;
  readonly onValidate?: Script;
  readonly onStore?: Script;
  readonly onRetrieve?: Script;
  readonly [key: string]: unknown;
};
export type Hook = "onCreate" | "onUpdate" | "onValidate" | "onStore" |
  "postCreate" | "postUpdate" | "onRetrieve" | "onRead" | "onDelete" |
  "postDelete" | "onSync";
export type ManagedType = {
  readonly name: string;
  readonly schema: {
    readonly properties: Readonly<Record<string, PropertyConfig>>;
    readonly [key: string]: unknown;
  };
  readonly actions?: Readonly<Record<string, Script>>;
  readonly [key: string]: unknown;
} & Partial<Readonly<Record<Hook, Script>>>;
export type ManagedConfig = {
  readonly objects: readonly ManagedType[];
  readonly [key: string]: unknown;
};
export type EndpointConfig = Script | {
  readonly type: "scripted";
  readonly script: Script;
  readonly [key: string]: unknown;
};
export type ConfigInput = {
  readonly managed: ManagedConfig;
  /** Keys are bare endpoint names, as in config/endpoint/<name>. */
  readonly endpoints?: Readonly<Record<string, EndpointConfig>>;
  /** Maps are partial unless the caller explicitly declares the namespace complete. */
  readonly complete?: { readonly managed?: boolean; readonly endpoints?: boolean };
  /**
   * The tenant's `config/repo.ds`, as read. Optional: it is how the emulator learns a managed
   * type's storage kind (`IdmConfig.storage`). Without it a generic type's number queries keep
   * the undecided faults the JSON-storage measurement would otherwise retire. Pass the whole
   * document: a partial one holding only `managed/*` makes every non-user type generic, including
   * types the tenant maps under `explicitMapping`, and nothing here can tell (review pf-01 P3-7).
   */
  readonly repo?: Readonly<Record<string, JsonValue>>;
  /**
   * The tenant's `config/sync`, as read (`{ mappings: [...] }`). Optional: it is how the emulator
   * learns which managed types are a mapping's source. A committed write on such a type runs the
   * mapping on AIC (implicit sync, docs/api/16-sync-mappings.md), which the emulator does not
   * model, so that write is the harness fault `idm-implicit-sync/<mapping>`. Without it every
   * write records the assumption that no mapping has the type as its source.
   */
  readonly sync?: Readonly<Record<string, JsonValue>>;
};
export type RestRequest = {
  method: "GET" | "POST" | "PUT" | "PATCH" | "DELETE";
  path: string;
  query?: Readonly<Record<string, JsonValue>>;
  headers?: Readonly<Record<string, string>>;
  body?: JsonValue;
};
export type Provenance = {
  kind: "measured" | "assumed" | "missing";
  behavior: string;
  source?: string;
};
export type RestResult = {
  emulation?: EmulationFailureInfo;
  status: number;
  body: JsonValue;
  /** Exact UTF-8 response bytes when the tenant produced non-JSON/empty output.
   * In that case body is null; transports must send rawBody verbatim. */
  rawBody?: string;
  provenance: readonly Provenance[];
};
/** Policy is deliberately a separate seam: plug in the tenant's policy.js later.
 * Throw the runner's HostCallError to reject a write with a CREST status/detail. */
export type PolicyEvaluator = (input: {
  object: Document;
  schema: ManagedType["schema"];
  request: RestRequest;
  internal: boolean;
}) => void | Promise<void>;
