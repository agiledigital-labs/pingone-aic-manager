/** Tenant-shaped config and authenticate REST contracts. No test framework dependencies. */
import type { JsonValue } from "../protocol.ts";
import type { RestResult } from "../idm/types.ts";
export type TreeNode = {
  readonly nodeType: string;
  readonly displayName?: string;
  readonly connections: Readonly<Record<string, string>>;
  readonly [key: string]: unknown;
};
export type TreeDocument = {
  readonly entryNodeId: string;
  readonly nodes: Readonly<Record<string, TreeNode>>;
  readonly innerTreeOnly?: boolean;
  readonly identityResource?: string;
  readonly [key: string]: unknown;
};
export type NodeDocument = {
  readonly script?: string;
  readonly outcomes?: readonly string[];
  readonly [key: string]: unknown;
};
export type ScriptDocument = {
  readonly script: string;
  readonly evaluatorVersion: string;
  readonly name?: string;
  readonly language?: string;
  readonly context?: string;
  readonly [key: string]: unknown;
};
export type JourneyConfigInput = {
  readonly realm: string;
  readonly trees: Readonly<Record<string, TreeDocument>>;
  /** Node REST documents, indexed by their type then id. */
  readonly nodes: Readonly<Record<string, Readonly<Record<string, NodeDocument>>>>;
  /** Script REST documents: script is base64 encoded, as on the tenant. */
  readonly scripts: Readonly<Record<string, ScriptDocument>>;
};
export type AuthenticateRequest = {
  readonly method: "POST";
  readonly path: string;
  readonly query: {
    readonly authIndexType: string;
    readonly authIndexValue: string;
    readonly [key: string]: string | readonly string[];
  };
  readonly body?: JsonValue;
  /** Raw transport input; parsing stays inside the audited request scope. */
  readonly rawBody?: string;
  readonly headers?: Readonly<Record<string, string[]>>;
  readonly cookies?: Readonly<Record<string, string>>;
  /**
   * Only the log records read these four. `url` is the full URL as the caller sent it, and is
   * what `http.request.path` carries; `transactionId` stands for the `x-forgerock-transactionid`
   * header; this field wins over the header.
   */
  readonly url?: string;
  readonly clientIp?: string;
  readonly userAgent?: string;
  readonly transactionId?: string;
};
/** A cookie the authenticate response sets, beside the session cookie. */
export type SetCookie = {
  readonly name: string;
  readonly value: string;
  /** HTTP date, as the tenant's `Expires` attribute. */
  readonly expires: string;
  readonly path: string;
  readonly secure: boolean;
  readonly httpOnly: boolean;
  readonly sameSite: "lax" | "strict" | "none";
};
/** What a node asks of the session AM creates on success. */
export type SessionDelta = {
  properties?: Record<string, string>;
  /** Minutes. */
  maxIdleTime?: number;
  maxSessionTime?: number;
  cookie?: SetCookie;
};
/** The Terms and Conditions the realm serves; a tenant setting, not node configuration. */
export type TermsAndConditions = {
  readonly version: string;
  readonly terms: string;
  readonly createDate: string;
};
export type AuthenticateResult = RestResult & {
  /** Set only on a successful journey that asked for session effects. */
  readonly session?: {
    readonly properties: Readonly<Record<string, string>>;
    readonly maxIdleTime?: number;
    readonly maxSessionTime?: number;
  };
  readonly cookies?: readonly SetCookie[];
};
/**
 * What AIC would put on the wire for the request, beside the parsed `options`
 * (docs/api/12): `bodyText` renders every JS number as a Java double (`1` is
 * `1.0`) and `undefined` as `null`, and is `""` when `options` has no `body`.
 * That was measured for object bodies sent as `Content-Type: application/json`
 * with plain-ASCII strings; anything else is labelled assumed, and `form` is
 * missing.
 */
export type HttpWire = {
  readonly bodyText: string;
  /**
   * Set when AIC chooses the Content-Type itself: a `form` request goes out as
   * `application/x-www-form-urlencoded` whatever header the script set (docs/api/12 §6), and
   * `bodyText` is then the encoded pairs.
   */
  readonly contentType?: string;
};
export type HttpMock = (
  url: string,
  options: { [key: string]: JsonValue },
  wire: HttpWire,
) =>
  | Promise<{
      status: number;
      body?: JsonValue;
      headers?: Record<string, string>;
    }>
  | { status: number; body?: JsonValue; headers?: Record<string, string> };
