/** Managed repository and measured lifecycle chains. Scripts own JVM execution only. */
import { createHmac, randomBytes, randomUUID } from "node:crypto";
import { isDeepStrictEqual } from "node:util";
import { HostCallError } from "../runner.ts";
import type { JsonValue } from "../protocol.ts";
import { applyPatch, document, fields, project } from "./document.ts";
import { IdmError, invalidInput, missing } from "./errors.ts";
import { fieldValue, matchesFilter, parseFilter } from "./filter.ts";
import type { Evidence } from "./provenance.ts";
import type { Invocation, Scripts } from "./scripts.ts";
import type { Document, ManagedType, PolicyEvaluator, RestRequest } from "./types.ts";

export type Operation = {
  request: RestRequest;
  context: Document;
  internal: boolean;
};
export type Reply = { status: number; body: JsonValue; rawBody?: string };

export function header(request: RestRequest, name: string): string | undefined {
  return Object.entries(request.headers ?? {}).find(([key]) => key.toLowerCase() === name.toLowerCase())?.[1];
}

export function revision(request: RestRequest): string | undefined {
  const value = header(request, "If-Match");
  if (value !== undefined) return value.replace(/^"|"$/g, "");
  const body = request.body;
  return body && !Array.isArray(body) && typeof body === "object" && typeof body._rev === "string" ? body._rev : undefined;
}

export class ManagedStore {
  private readonly cookieKey = randomBytes(32);
  private readonly records: Map<string, Document>;
  private readonly scripts: Scripts;
  private readonly evidence: Evidence;
  private readonly policy: PolicyEvaluator | undefined;
  constructor(
    records: Map<string, Document>,
    scripts: Scripts,
    evidence: Evidence,
    policy?: PolicyEvaluator,
  ) {
    this.records = records;
    this.scripts = scripts;
    this.evidence = evidence;
    this.policy = policy;
  }

  async dispatch(type: ManagedType, id: string | undefined, op: Operation): Promise<Reply> {
    const { request } = op;
    const action = request.query?._action;
    const noneMatch = header(request, "If-None-Match");
    if (noneMatch !== undefined && noneMatch !== "*") missing(this.evidence, "If-None-Match other than star");
    for (const [name, property] of Object.entries(type.schema.properties)) {
      if (property.type === "relationship" ||
        (property.type === "array" && (property.items as { type?: string } | undefined)?.type === "relationship")) {
        missing(this.evidence, `managed relationship: ${type.name}/${name}`);
      }
    }
    this.evidence.measured("managed CRUD status and response shapes", "docs/api/10-managed-objects.md");
    if (request.method === "GET") {
      if (id !== undefined) return { status: 200, body: await this.read(type, id, op) };
      return { status: 200, body: await this.query(type, op) };
    }
    if (request.method === "POST" && action !== undefined && action !== "create") return this.action(type, id, op);
    if (request.method === "POST") {
      if (action === undefined) this.evidence.assumed("bare managed POST maps to create");
      const body = document(request.body);
      return this.create(type, id ?? (typeof body._id === "string" ? body._id : randomUUID()), op);
    }
    if (id === undefined) missing(this.evidence, `managed collection method ${request.method}`);
    if (request.method === "PUT") {
      if (revision(request) !== undefined) return this.update(type, id, op, false);
      const prepared = await this.prepare(type, id, op, null, "create");
      if (this.records.has(this.key(type, id))) {
        if (header(request, "If-None-Match") === "*") throw this.exists();
        this.evidence.measured("unconditional PUT discards create-chain mutations before update");
        return this.update(type, id, op, true);
      }
      return this.commitCreate(type, id, op, prepared);
    }
    if (request.method === "PATCH") return this.update(type, id, op, false);
    return this.delete(type, id, op);
  }

  private key(type: ManagedType, id: string): string { return `managed/${type.name}/${id}`; }

  private stored(type: ManagedType, id: string): Document {
    const record = this.records.get(this.key(type, id));
    if (!record) {
      this.evidence.assumed("missing-record message omits tenant directory DN");
      throw new IdmError(404, "No Such Entry");
    }
    return structuredClone(record);
  }

  private exists(): IdmError {
    this.evidence.assumed("duplicate-create message omits tenant directory DN");
    return new IdmError(412, "Entry Already Exists");
  }

  private invocation(type: ManagedType, id: string, op: Operation, method: string, object: Document | null, oldObject: Document | null, newObject: Document | null): Invocation {
    const path = this.key(type, id);
    const query = op.request.query ?? {};
    const request: Document = { method, resourcePath: id, fields: fields(query._fields),
      additionalParameters: Object.fromEntries(Object.entries(query).filter(([key]) => !key.startsWith("_"))) };
    if (["create", "update", "action"].includes(method)) request.content = op.request.body ?? null;
    if (method === "create") request.newResourceId = id;
    if (["update", "patch", "delete"].includes(method)) request.revision = revision(op.request) ?? null;
    if (method === "patch") request.patchOperations = op.request.body ?? [];
    if (method === "action") request.action = query._action ?? null;
    return { request, context: op.context, resourceName: path, object, oldObject, newObject };
  }

  private checkRevision(object: Document, request: RestRequest): void {
    const expected = revision(request);
    if (expected !== undefined && expected !== "*" && expected !== object._rev) {
      this.evidence.assumed("revision mismatch message and precondition timing");
      throw new IdmError(412, "Revision mismatch");
    }
  }

  private materialize(type: ManagedType, object: Document): Document {
    this.evidence.assumed("generic scalar/array materialization follows measured user null/array shape");
    for (const [name, config] of Object.entries(type.schema.properties)) {
      if (!Object.hasOwn(object, name)) Object.defineProperty(object, name, {
        value: config.type === "array" ? [] : null, writable: true, enumerable: true, configurable: true,
      });
    }
    return object;
  }

  private async read(type: ManagedType, id: string, op: Operation, readHook = true, selectors = fields(op.request.query?._fields)): Promise<Document> {
    const object = this.stored(type, id);
    if (!readHook) this.evidence.assumed("executeOnRetrieve request.method is query");
    this.evidence.measured(readHook ? "managed read hook order and bindings" : "query executeOnRetrieve hook order and bindings");
    const invocation = this.invocation(type, id, op, readHook ? "read" : "query", object, null, null);
    await this.scripts.hook(type, "onRetrieve", invocation);
    await this.scripts.properties(type, "onRetrieve", invocation, selectors);
    if (readHook) await this.scripts.hook(type, "onRead", invocation);
    if (type.name === "alpha_user") {
      this.evidence.measured("alpha_user password is write-only", "docs/api/10-managed-objects.md");
      delete object.password;
    }
    return project(object, selectors, this.evidence);
  }

  private async prepare(type: ManagedType, id: string, op: Operation, oldObject: Document | null, method: "create" | "update"): Promise<Document> {
    const object = op.request.method === "PATCH" && oldObject ?
      applyPatch(oldObject, op.request.body, this.evidence) : document(op.request.body);
    this.evidence.assumed("undeclared attribute checking applies to external and internal writes");
    const undeclared = Object.keys(object).filter((key) => key !== "_id" && key !== "_rev" && !Object.hasOwn(type.schema.properties, key));
    if (undeclared.length) {
      this.evidence.measured("undeclared request attributes are rejected", "docs/api/10-managed-objects.md");
      throw new IdmError(400, `Request content includes undeclared attributes: ${JSON.stringify(undeclared)}`);
    }
    if (method === "create") {
      this.evidence.measured("declared defaults apply on create before policy", "docs/api/10-managed-objects.md");
      this.evidence.assumed("defaults apply before create hooks; non-scalar defaults use cloned JSON values");
      for (const [name, config] of Object.entries(type.schema.properties)) {
        if (!Object.hasOwn(object, name) && Object.hasOwn(config, "default")) {
          Object.defineProperty(object, name, { value: structuredClone(config.default) as JsonValue,
            writable: true, enumerable: true, configurable: true });
        }
      }
    }
    this.evidence.measured(`managed ${method} hook order and bindings`);
    const invocation = this.invocation(type, id, op, op.request.method === "PATCH" ? "patch" : method, object, oldObject, object);
    await this.scripts.hook(type, method === "create" ? "onCreate" : "onUpdate", invocation);
    await this.scripts.properties(type, "onValidate", invocation);
    await this.scripts.hook(type, "onValidate", invocation);
    if (this.policy) {
      this.evidence.assumed("caller-supplied policy evaluator");
      try { await this.policy({ object, schema: type.schema, request: op.request, internal: op.internal }); }
      catch (error) {
        if (error instanceof HostCallError) throw new IdmError(error.code, error.message, error.detail);
        throw error;
      }
    } else this.evidence.assumed("policy not evaluated");
    this.evidence.measured("property onStore always replaces the property including absent and undefined values");
    await this.scripts.properties(type, "onStore", invocation);
    await this.scripts.hook(type, "onStore", invocation);
    return object;
  }

  private async create(type: ManagedType, id: string, op: Operation): Promise<Reply> {
    const object = await this.prepare(type, id, op, null, "create");
    if (this.records.has(this.key(type, id))) throw this.exists();
    return this.commitCreate(type, id, op, object);
  }

  private async commitCreate(type: ManagedType, id: string, op: Operation, object: Document): Promise<Reply> {
    this.evidence.measured("server-assigned managed ids are UUIDs", "docs/api/10-managed-objects.md");
    this.evidence.assumed("revisions are monotonic decimal strings starting at zero");
    const record = this.materialize(type, { ...object, _id: id, _rev: "0" });
    this.records.set(this.key(type, id), structuredClone(record));
    await this.afterWrite(type, id, op, record, null, "create");
    return { status: 201, body: project(record, fields(op.request.query?._fields), this.evidence) };
  }

  private async update(type: ManagedType, id: string, op: Operation, skipRead: boolean): Promise<Reply> {
    const oldObject = this.stored(type, id);
    if (!skipRead) {
      await this.read(type, id, op, true, []);
      this.evidence.measured("write pre-read mutations are discarded");
    }
    this.checkRevision(oldObject, op.request);
    if (op.request.method === "PATCH") {
      const patched = applyPatch(oldObject, op.request.body, this.evidence);
      if (isDeepStrictEqual(oldObject, patched)) {
        this.evidence.measured("no-change PATCH only runs read hooks");
        return { status: 200, body: project(oldObject, fields(op.request.query?._fields), this.evidence) };
      }
    } else this.evidence.assumed("equal-content PUT still runs the write chain");
    const object = await this.prepare(type, id, op, oldObject, "update");
    if (this.records.get(this.key(type, id))?._rev !== oldObject._rev) {
      this.evidence.assumed("concurrent writes are checked again after hooks");
      throw new IdmError(412, "Revision mismatch");
    }
    this.evidence.assumed("revisions are monotonic decimal strings starting at zero");
    const record = this.materialize(type, { ...object, _id: id, _rev: String(Number(oldObject._rev) + 1) });
    this.records.set(this.key(type, id), structuredClone(record));
    await this.afterWrite(type, id, op, record, oldObject, "update");
    this.evidence.assumed("managed update and patch return the stored object with status 200");
    return { status: 200, body: project(record, fields(op.request.query?._fields), this.evidence) };
  }

  private async afterWrite(type: ManagedType, id: string, op: Operation, record: Document, oldObject: Document | null, method: "create" | "update"): Promise<void> {
    const invocation = this.invocation(type, id, op, op.request.method === "PATCH" ? "patch" : method, record, oldObject, record);
    this.evidence.assumed("post-hook mutations affect responses only");
    try {
      await this.scripts.hook(type, method === "create" ? "postCreate" : "postUpdate", invocation);
      await this.scripts.hook(type, "onSync", { ...invocation, object: null });
    } catch (error) {
      this.evidence.assumed("post-hook failure after commit keeps the write");
      throw error;
    }
  }

  private async delete(type: ManagedType, id: string, op: Operation): Promise<Reply> {
    const object = this.stored(type, id);
    this.checkRevision(object, op.request);
    this.evidence.measured("managed delete hook order; postDelete and onSync object is null");
    const invocation = this.invocation(type, id, op, "delete", object, object, null);
    await this.scripts.hook(type, "onDelete", invocation);
    if (this.records.get(this.key(type, id))?._rev !== object._rev) {
      this.evidence.assumed("concurrent writes are checked again after hooks");
      throw new IdmError(412, "Revision mismatch");
    }
    this.records.delete(this.key(type, id));
    this.evidence.assumed("post-hook mutations affect responses only");
    try {
      await this.scripts.hook(type, "postDelete", { ...invocation, object: null });
      await this.scripts.hook(type, "onSync", { ...invocation, object: null });
    } catch (error) {
      this.evidence.assumed("post-hook failure after commit keeps the write");
      throw error;
    }
    return { status: 200, body: project(object, fields(op.request.query?._fields), this.evidence) };
  }

  private async action(type: ManagedType, id: string | undefined, op: Operation): Promise<Reply> {
    if (id === undefined) {
      this.evidence.measured("managed collection action rejection");
      throw new IdmError(400, "Expecting String containing one of: patch triggerSyncCheck updateLastSync");
    }
    const name = op.request.query?._action;
    const script = typeof name === "string" ? type.actions?.[name] : undefined;
    if (!script) missing(this.evidence, `managed object action: ${String(name)}`);
    const object = await this.read(type, id, op);
    this.evidence.measured("object action reads twice and discards script completion");
    this.evidence.assumed("custom action receives object, request, context and resourceName");
    const invocation = this.invocation(type, id, op, "action", object, null, null);
    await this.scripts.evaluate(script, { object, request: invocation.request, context: op.context, resourceName: invocation.resourceName }, "action");
    return { status: 200, body: await this.read(type, id, op) };
  }

  private async query(type: ManagedType, op: Operation): Promise<Document> {
    const query = op.request.query ?? {};
    const kinds = ["_queryId", "_queryExpression", "_queryFilter"].filter((key) => query[key] !== undefined);
    if (kinds.length !== 1) {
      this.evidence.measured("query requires exactly one query selector");
      throw new IdmError(400, "Only one of [_queryId, _queryExpression, _queryFilter] is supported; multiple detected");
    }
    if (query._queryExpression !== undefined) {
      this.evidence.measured("DS query expressions are unsupported");
      throw new IdmError(405, "Query Expressions are not supported when using DS as the repo");
    }
    if (query._queryId !== undefined) missing(this.evidence, `named _queryId: ${String(query._queryId)}`);
    if (typeof query._queryFilter !== "string") throw new IdmError(400, "A value could not be parsed as a valid query filter");
    this.evidence.measured("CREST filter syntax and documented parse failures", "docs/api/10-managed-objects.md");
    this.evidence.assumed("CREST comparison uses case-sensitive strings and existential array matching");
    const filter = parseFilter(query._queryFilter);
    let rows = [...this.records.entries()].filter(([key, row]) => key.startsWith(`managed/${type.name}/`) && matchesFilter(filter, row)).map(([, row]) => structuredClone(row));
    if (query._sortKeys !== undefined) {
      this.evidence.measured("nested query sort keys", "docs/api/10-managed-objects.md");
      this.evidence.assumed("sort nulls first and break ties by insertion order");
      const keys = fields(query._sortKeys);
      rows.sort((a, b) => {
        for (const sort of keys) {
          const descending = sort.startsWith("-");
          const path = sort.replace(/^[+-]/, "");
          const av = fieldValue(a, path) ?? null;
          const bv = fieldValue(b, path) ?? null;
          const cmp = av === bv ? 0 : av === null ? -1 : bv === null ? 1 : av < bv ? -1 : 1;
          if (cmp !== 0) return descending ? -cmp : cmp;
        }
        return 0;
      });
    }
    if (query._pagedResultsCookie === null) {
      this.evidence.measured("explicit null paging cookie yields 500", "docs/api/10-managed-objects.md");
      throw new IdmError(500, "/_pagedResultsCookie: Expecting a value");
    }
    const sign = (offset: string): string => createHmac("sha256", this.cookieKey)
      .update(JSON.stringify([type.name, query._queryFilter, query._sortKeys, offset])).digest("base64url");
    let offset = integer(query._pagedResultsOffset, 0);
    if (query._pagedResultsCookie !== undefined && query._pagedResultsCookie !== "") {
      if (typeof query._pagedResultsCookie !== "string") throw invalidInput("Invalid paging cookie");
      const [encoded, signature] = query._pagedResultsCookie.split(".");
      if (!encoded || signature !== sign(encoded)) throw invalidInput("Invalid paging cookie");
      offset = integer(Buffer.from(encoded, "base64url").toString(), 0);
    }
    const size = integer(query._pageSize, 0);
    this.evidence.measured("paging continuation cookies and query envelope", "docs/api/10-managed-objects.md");
    this.evidence.assumed("opaque signed offset cookies re-evaluate the query; concurrent query mutation stability is not modeled");
    this.evidence.assumed("zero page size is unbounded");
    const nextOffset = offset + size;
    const encoded = Buffer.from(String(nextOffset)).toString("base64url");
    const cookie = size > 0 && nextOffset < rows.length ? `${encoded}.${sign(encoded)}` : null;
    rows = rows.slice(offset, size > 0 ? offset + size : undefined);
    const selectors = fields(query._fields);
    if (String(query.executeOnRetrieve) === "true") {
      const transformed: Document[] = [];
      for (const row of rows) transformed.push(await this.read(type, String(row._id), op, false, selectors));
      rows = transformed;
    } else {
      this.evidence.measured("ordinary managed query runs no hooks");
      rows = rows.map((row) => {
        if (type.name === "alpha_user") delete row.password;
        return project(row, selectors, this.evidence);
      });
    }
    const result: Document = { result: rows, resultCount: rows.length, pagedResultsCookie: cookie,
      totalPagedResults: -1, totalPagedResultsPolicy: "NONE" };
    this.evidence.measured("query counts and internal omission of remainingPagedResults", "docs/api/10-managed-objects.md");
    if (!op.internal) result.remainingPagedResults = -1;
    return result;
  }
}

export function integer(value: JsonValue | undefined, fallback: number): number {
  if (value === undefined) return fallback;
  const number = typeof value === "number" || typeof value === "string" ? Number(value) : NaN;
  if (!Number.isSafeInteger(number) || number < 0) throw invalidInput("Invalid non-negative integer query parameter");
  return number;
}
