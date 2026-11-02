/** Tenant-shaped config and authenticate REST contracts. No test framework dependencies. */
import type { JsonValue } from "../protocol.ts";
import type { RestResult } from "../idm/types.ts";
export type TreeNode = { readonly nodeType: string; readonly displayName?: string; readonly connections: Readonly<Record<string, string>>; readonly [key: string]: unknown };
export type TreeDocument = {
  readonly entryNodeId: string;
  readonly nodes: Readonly<Record<string, TreeNode>>;
  readonly innerTreeOnly?: boolean;
  readonly identityResource?: string;
  readonly [key: string]: unknown;
};
export type NodeDocument = { readonly script?: string; readonly outcomes?: readonly string[]; readonly [key: string]: unknown };
export type ScriptDocument = { readonly script: string; readonly evaluatorVersion: string; readonly name?: string; readonly language?: string; readonly context?: string; readonly [key: string]: unknown };
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
  readonly query: { readonly authIndexType: string; readonly authIndexValue: string; readonly [key: string]: string | readonly string[] };
  readonly body?: JsonValue;
  readonly headers?: Readonly<Record<string, string[]>>;
  readonly cookies?: Readonly<Record<string, string>>;
};
export type AuthenticateResult = RestResult;
export type HttpMock = (url: string, options: { [key: string]: JsonValue }) => Promise<{ status: number; body?: JsonValue; headers?: Record<string, string> }> | { status: number; body?: JsonValue; headers?: Record<string, string> };
