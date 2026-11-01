/** REST and config contracts for the IDM core. No test framework or transport types. */
import type { JsonValue } from "../protocol.ts";

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
