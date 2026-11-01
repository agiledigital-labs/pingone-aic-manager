/** Scripted endpoint CREST request construction and measured completion envelopes. */
import type { JsonValue } from "../protocol.ts";
import { fields, isDocument, own } from "./document.ts";
import { IdmError, missing } from "./errors.ts";
import { header, integer, revision, type Operation, type Reply } from "./managed.ts";
import type { Evidence } from "./provenance.ts";
import type { Scripts } from "./scripts.ts";
import type { Document, EndpointConfig, Script } from "./types.ts";

export async function endpoint(config: EndpointConfig, subpath: string, op: Operation, scripts: Scripts, evidence: Evidence, forcedMethod?: string): Promise<Reply> {
  const { request: rest } = op;
  const query = rest.query ?? {};
  const isQuery = ["_queryFilter", "_queryId", "_queryExpression"].some((key) => own(query, key) !== undefined);
  const method = forcedMethod ?? (rest.method === "GET" ? isQuery ? "query" : "read" :
    rest.method === "POST" ? query._action !== undefined && query._action !== "create" ? "action" : "create" :
    rest.method === "PUT" ? header(rest, "If-Match") === undefined ? "create" : "update" :
    rest.method === "PATCH" ? "patch" : "delete");
  const resourcePath = rest.method === "PUT" && method === "create" ? "" : subpath;
  const request: Document = {
    method, fields: fields(query._fields), resourcePath,
    additionalParameters: Object.fromEntries(Object.entries(query).filter(([key]) => !key.startsWith("_"))),
  };
  if (["action", "create", "update"].includes(method)) request.content = rest.body ?? null;
  if (method === "action") request.action = query._action ?? null;
  if (method === "create") request.newResourceId = subpath.split("/").at(-1) || null;
  if (["update", "patch", "delete"].includes(method)) request.revision = revision(rest) ?? null;
  if (method === "patch") request.patchOperations = rest.body ?? [];
  if (method === "query") {
    Object.assign(request, {
      queryFilter: query._queryFilter ?? null, queryId: query._queryId ?? null,
      queryExpression: query._queryExpression ?? null,
      pageSize: integer(query._pageSize, 0), pagedResultsOffset: integer(query._pagedResultsOffset, 0),
      pagedResultsCookie: query._pagedResultsCookie ?? null, sortKeys: fields(query._sortKeys),
    });
    evidence.assumed("endpoint query defaults and sortKeys are plain JSON values");
  }
  evidence.measured("endpoint method request keys and resource paths");
  const script = Object.hasOwn(config, "script") ? (config as { script: Script }).script : config as Script;
  const response = await scripts.evaluate(script, { request, context: op.context, resourceName: resourcePath }, "endpoint");
  const value = op.internal && Object.hasOwn(response, "internalValue") ? response.internalValue ?? null : response.value;
  if (method === "action") {
    evidence.measured("endpoint action returns completion without _id");
    return { status: 200, body: value };
  }
  if (method === "query") {
    evidence.measured("endpoint query arrays wrap as results and id stamps _id");
    let rows: JsonValue[];
    if (value === null) rows = [];
    else if (Array.isArray(value)) rows = value.map((row) => isDocument(row) && row._id === undefined && row.id !== undefined ? { ...row, _id: row.id } : row);
    else if (isDocument(value)) throw new IdmError(500, "Script returned unexpected query result structure of type class java.util.HashMap");
    else missing(evidence, `endpoint query completion type: ${typeof value}`);
    evidence.assumed("endpoint array query paging metadata uses NONE and unknown totals");
    const body: Document = { result: rows, resultCount: rows.length, pagedResultsCookie: null,
      remainingPagedResults: -1, totalPagedResults: -1, totalPagedResultsPolicy: "NONE" };
    if (op.internal) delete body.remainingPagedResults;
    return { status: 200, body };
  }
  if (value === null) {
    if (method === "read") {
      evidence.measured("endpoint null read is 404 script returned null");
      throw new IdmError(404, "script returned null");
    }
    missing(evidence, `endpoint ${method} null completion`);
  }
  const id = subpath.split("/").at(-1) ?? "";
  if (!isDocument(value)) {
    if (method === "read" && (Array.isArray(value) || typeof value === "string" || typeof value === "number")) {
      evidence.measured("endpoint scalar/array read returns truncated invalid JSON bytes");
      if (id !== "") evidence.assumed("truncated endpoint read stamps the final subpath segment");
      return { status: 200, body: null, rawBody: `{"_id":${JSON.stringify(id)}` };
    }
    missing(evidence, `endpoint ${method} non-object completion`);
  }
  evidence.measured("endpoint resource completion stamps last path segment as _id");
  return { status: method === "create" ? 201 : 200, body: { ...value, _id: id } };
}
