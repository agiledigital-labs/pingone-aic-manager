import { EmulationFailure, emulationFailure } from "../emulation.ts";
/** Managed repository and measured lifecycle chains. Scripts own JVM execution only. */
import { createHmac, randomBytes, randomUUID } from "node:crypto";
import { isDeepStrictEqual } from "node:util";
import { HostCallError } from "../runner.ts";
import type { JsonValue } from "../protocol.ts";
import { checkCapabilities } from "./capabilities.ts";
import { applyPatch, document, fields, own, project } from "./document.ts";
import { IdmError, invalidInput, missing } from "./errors.ts";
import { type Operator, type RuleDimension, type StringPolicy, type StringRule, classifyFilter, fieldRule, fieldValue, illegalAssertion, matchesFilter } from "./filter.ts";
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
  return body && !Array.isArray(body) && typeof body === "object" && typeof own(body, "_rev") === "string" ? own(body, "_rev") as string : undefined;
}

export class ManagedStore {
  private cookieKey = randomBytes(32);
  // Body syntax is unmeasured: recognize our opaque tokens by issuance, not a guessed regex.
  private passwordTokens = new Set<string>();
  private readonly records: Map<string, Document>;
  /** Array properties `materialize` injected per record, which a query reads as absent (round-5 P3-6). */
  private materialized = new Map<string, Set<string>>();
  private readonly scripts: Scripts;
  private readonly evidence: Evidence;
  private readonly policy: PolicyEvaluator | undefined;
  /** Whether a type's top-level property is JSON in DS (`IdmConfig.jsonStored`). */
  private readonly jsonStored: (type: string, field: string) => boolean;
  /** The sync mappings with this type as source (`IdmConfig.syncMappings`). */
  private readonly syncMappings: (type: string) => readonly string[] | undefined;
  constructor(
    records: Map<string, Document>,
    scripts: Scripts,
    evidence: Evidence,
    policy?: PolicyEvaluator,
    jsonStored: (type: string, field: string) => boolean = () => false,
    syncMappings: (type: string) => readonly string[] | undefined = () => undefined,
  ) {
    this.records = records;
    this.scripts = scripts;
    this.evidence = evidence;
    this.policy = policy;
    this.jsonStored = jsonStored;
    this.syncMappings = syncMappings;
  }

  /**
   * Implicit sync, checked immediately before a write commits. On AIC a committed managed write
   * on a mapping's source runs that mapping (its onCreate/onUpdate/onDelete/onLink scripts and
   * the target write) synchronously, measured 2026-06-19 (docs/api/16-sync-mappings.md, "Recon vs.
   * implicit (live) sync"). Nothing here runs a mapping, so a write that would is a fault; it is
   * raised before the store changes.
   */
  private implicitSync(type: ManagedType, method: string): void {
    const mappings = this.syncMappings(type.name);
    if (mappings === undefined) {
      this.evidence.assumed("no config/sync supplied: no sync mapping is assumed to have this type as its source, so no implicit sync runs");
      return;
    }
    if (mappings.length === 0) {
      this.evidence.assumed("the supplied config/sync has no mapping with this type as its source, so no implicit sync runs");
      return;
    }
    missing(this.evidence, `implicit sync: managed/${type.name} is the source of sync mapping${mappings.length > 1 ? "s" : ""} ${mappings.join(", ")}; ` +
      `a committed ${method} runs ${mappings.length > 1 ? "them" : "it"} on AIC (docs/api/16-sync-mappings.md), and the emulator does not model sync`,
    `idm-implicit-sync/${mappings[0]}`);
  }

  /** Forks retain continuation credentials; offset cookies use each fork's current rows. */
  inheritCookies(parent: ManagedStore): void { this.cookieKey = Buffer.from(parent.cookieKey); }

  /** A fork's records are copies of the parent's, so it copies which of their arrays were injected. */
  inheritMaterialized(parent: ManagedStore): void {
    this.materialized = new Map([...parent.materialized].map(([key, names]) => [key, new Set(names)]));
  }

  /** Token validity knowledge is shared by a fork family; record state remains isolated. */
  inheritPasswordTokens(parent: ManagedStore): void { this.passwordTokens = parent.passwordTokens; }

  async dispatch(type: ManagedType, id: string | undefined, op: Operation): Promise<Reply> {
    const { request } = op;
    const action = request.query?._action;
    // Configuration shape checks ran before routing. Finish known absence and
    // implemented rejections before inspecting storage capabilities.
    if (request.method === "POST" && id === undefined && action !== undefined && action !== "create") return this.action(type, id, op);
    if (id !== undefined && (["GET", "PATCH", "DELETE"].includes(request.method) ||
        (request.method === "PUT" && revision(request) !== undefined))) {
      const existing = this.stored(type, id);
      if (request.method === "DELETE") this.checkRevision(existing, request);
    }
    const noneMatch = header(request, "If-None-Match");
    if (noneMatch !== undefined && noneMatch !== "*") missing(this.evidence, "If-None-Match other than star");
    if (request.method !== "GET") checkCapabilities(type, request.method !== "DELETE", this.evidence, this.policy !== undefined);
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

  private materialize(type: ManagedType, object: Document, injected?: Set<string>): Document {
    this.evidence.assumed("generic scalar/array materialization follows measured user null/array shape");
    for (const [name, config] of Object.entries(type.schema.properties)) {
      if (!Object.hasOwn(object, name)) {
        Object.defineProperty(object, name, {
          value: config.type === "array" ? [] : null, writable: true, enumerable: true, configurable: true,
        });
        if (config.type === "array") injected?.add(name);
      }
    }
    return object;
  }

  /**
   * Which arrays of the record now stored at `key` are still the `[]` this emulator injected: those
   * injected by this write, and those injected earlier that a PATCH left alone. An unset
   * multi-valued attribute reads back as `[]` (doc 10), so a query must see it as absent; `pr` on
   * an explicitly written `[]` stays unmeasured.
   */
  private remember(key: string, record: Document, injected: Set<string>, patch?: JsonValue): void {
    const named = new Set((Array.isArray(patch) ? patch : []).map((op) =>
      op && typeof op === "object" && !Array.isArray(op) ? String(op.field ?? "").replace(/^\//, "").split("/")[0] : ""));
    for (const name of patch === undefined ? [] : this.materialized.get(key) ?? []) {
      const value = record[name];
      if (!named.has(name) && Array.isArray(value) && value.length === 0) injected.add(name);
    }
    if (injected.size) this.materialized.set(key, injected); else this.materialized.delete(key);
  }

  /** The record as a query reads it: injected arrays absent. */
  private queried(key: string, row: Document): Document {
    const names = this.materialized.get(key);
    if (!names) return row;
    const view: Document = { ...row };
    for (const name of names) if (Array.isArray(view[name]) && (view[name] as JsonValue[]).length === 0) delete view[name];
    return view;
  }

  private async read(type: ManagedType, id: string, op: Operation, readHook = true, selectors = fields(op.request.query?._fields)): Promise<Document> {
    const object = this.stored(type, id);
    checkCapabilities(type, false, this.evidence, this.policy !== undefined);
    if (!readHook) this.evidence.assumed("executeOnRetrieve request.method is query");
    this.evidence.measured(readHook ? "managed read hook order and bindings" : "query executeOnRetrieve hook order and bindings");
    const invocation = this.invocation(type, id, op, readHook ? "read" : "query", object, null, null);
    await this.scripts.hook(type, "onRetrieve", invocation);
    await this.scripts.properties(type, "onRetrieve", invocation, selectors);
    if (readHook) await this.scripts.hook(type, "onRead", invocation);
    return this.projectVisible(type, object, selectors, op);
  }

  private encodePassword(type: ManagedType, object: Document): void {
    if (!Object.hasOwn(object, "password")) return;
    if (type.name === "bravo_user") missing(this.evidence, "bravo_user password semantics");
    if (type.name !== "alpha_user") return;
    if (typeof object.password !== "string") missing(this.evidence, "alpha_user password without measured string value");
    this.evidence.assumed("opaque password body uses 70 hex characters from a randomly keyed HMAC-SHA512; commit timing is unmeasured");
    if (object.password.startsWith("{PBKDF2-HMAC-SHA256}")) {
      if (!this.passwordTokens.has(object.password)) missing(this.evidence, "alpha_user password prefixed body is unmeasured");
      this.evidence.measured("alpha_user encoded password survives update, password PATCH and cross-record create verbatim", "orchestrator p25, 2026-11-02; docs/api/10-managed-objects.md");
      return;
    }
    if (/^\{[^}]*\}/.test(object.password)) missing(this.evidence, "alpha_user password scheme is unmeasured");
    object.password = "{PBKDF2-HMAC-SHA256}" + createHmac("sha512", randomBytes(32)).update(object.password).digest("hex").slice(0, 70);
    this.passwordTokens.add(object.password);
  }

  private projectVisible(type: ManagedType, object: Document, selectors: string[], op: Operation): Document {
    if (type.name === "bravo_user" && Object.hasOwn(object, "password")) missing(this.evidence, "bravo_user password semantics");
    const result = project(object, selectors, this.evidence);
    if (type.name !== "alpha_user") return result;
    if (op.internal) {
      if (Object.hasOwn(result, "password")) {
        if (typeof result.password !== "string") missing(this.evidence, "alpha_user password without measured string value");
        if (!result.password.startsWith("{PBKDF2-HMAC-SHA256}")) missing(this.evidence, "alpha_user password representation after retrieval hooks");
        this.evidence.measured("alpha_user internal read/query exposes encoded password prefix", "orchestrator p22/p23, 2026-11-02; docs/api/10-managed-objects.md");
        this.evidence.assumed("opaque password body uses 70 hex characters from a randomly keyed HMAC-SHA512; commit timing is unmeasured");
      }
    } else {
      this.evidence.measured("alpha_user HTTP GET omits password", "docs/api/10-managed-objects.md");
      if (selectors.includes("password")) this.evidence.assumed("external password projection also omits the field");
      delete result.password;
    }
    return result;
  }

  private patch(type: ManagedType, object: Document, op: Operation): Document {
    const operations = op.request.body;
    const normalized = type.name === "alpha_user" && op.internal && Array.isArray(operations) ? operations.map((raw) => {
      if (raw && typeof raw === "object" && !Array.isArray(raw) && raw.operation === "replace" && raw.field === "password") {
        this.evidence.measured("alpha_user internal password PATCH accepts bare password field", "orchestrator p25, 2026-11-02; docs/api/10-managed-objects.md");
        return { ...raw, field: "/password" };
      }
      return raw;
    }) : operations;
    return applyPatch(object, normalized, this.evidence);
  }

  private async prepare(type: ManagedType, id: string, op: Operation, oldObject: Document | null, method: "create" | "update"): Promise<Document> {
    const object = op.request.method === "PATCH" && oldObject ?
      this.patch(type, oldObject, op) : document(op.request.body);
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
        const fatal = emulationFailure(error);
        if (fatal) throw new EmulationFailure(fatal);
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
    if (["alpha_user", "bravo_user"].includes(type.name) && !/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(id)) {
      this.evidence.measured("alpha_user and bravo_user ids require UUID syntax", "orchestrator p21, 2026-11-02; docs/api/10-managed-objects.md");
      this.evidence.assumed("invalid user id diagnostic substitutes realm placeholder and omits unmeasured suffix");
      throw new IdmError(400, `Invalid Attribute Syntax: Entry "fr-idm-uuid=${id},ou=user,o=<realm>,o=root,ou=identities" contains a value "${id}"`);
    }
    this.evidence.measured("server-assigned managed ids are UUIDs", "docs/api/10-managed-objects.md");
    this.evidence.assumed("revisions are monotonic decimal strings starting at zero");
    this.evidence.assumed("recreated records restart revision at 0");
    this.implicitSync(type, "create");
    this.encodePassword(type, object);
    const injected = new Set<string>();
    const record = this.materialize(type, { ...object, _id: id, _rev: "0" }, injected);
    this.records.set(this.key(type, id), structuredClone(record));
    this.remember(this.key(type, id), record, injected);
    await this.afterWrite(type, id, op, record, null, "create");
    return { status: 201, body: project(record, fields(op.request.query?._fields), this.evidence) };
  }

  private async update(type: ManagedType, id: string, op: Operation, skipRead: boolean): Promise<Reply> {
    const original = this.records.get(this.key(type, id));
    const oldObject = this.stored(type, id);
    if (!skipRead) {
      this.evidence.assumed("update pre-read and retrieval hooks run before revision validation, including stale revisions");
      await this.read(type, id, op, true, []);
      this.evidence.measured("write pre-read mutations are discarded");
      if (this.records.get(this.key(type, id)) !== original) throw new IdmError(412, "Revision mismatch");
    }
    this.checkRevision(oldObject, op.request);
    if (op.request.method === "PATCH") {
      const patched = this.patch(type, oldObject, op);
      if (isDeepStrictEqual(oldObject, patched)) {
        this.encodePassword(type, patched);
        this.evidence.measured("no-change PATCH only runs read hooks");
        return { status: 200, body: project(oldObject, fields(op.request.query?._fields), this.evidence) };
      }
    } else this.evidence.assumed("equal-content PUT still runs the write chain");
    const object = await this.prepare(type, id, op, oldObject, "update");
    if (this.records.get(this.key(type, id)) !== original || this.records.get(this.key(type, id))?._rev !== oldObject._rev) {
      this.evidence.assumed("concurrent writes are checked again after hooks");
      throw new IdmError(412, "Revision mismatch");
    }
    this.evidence.assumed("revisions are monotonic decimal strings starting at zero");
    this.implicitSync(type, "update");
    this.encodePassword(type, object);
    const injected = new Set<string>();
    const record = this.materialize(type, { ...object, _id: id, _rev: String(Number(oldObject._rev) + 1) }, injected);
    this.records.set(this.key(type, id), structuredClone(record));
    this.remember(this.key(type, id), record, injected, op.request.method === "PATCH" ? op.request.body ?? [] : undefined);
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
    const original = this.records.get(this.key(type, id));
    const storedRevision = object._rev;
    const invocation = this.invocation(type, id, op, "delete", object, object, null);
    await this.scripts.hook(type, "onDelete", invocation);
    if (this.records.get(this.key(type, id)) !== original || this.records.get(this.key(type, id))?._rev !== storedRevision) {
      this.evidence.assumed("concurrent writes are checked again after hooks");
      throw new IdmError(412, "Revision mismatch");
    }
    this.implicitSync(type, "delete");
    this.records.delete(this.key(type, id));
    this.materialized.delete(this.key(type, id));
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
      const name = op.request.query?._action;
      if (["patch", "triggerSyncCheck", "updateLastSync"].includes(String(name))) missing(this.evidence, `managed collection action: ${String(name)}`);
      this.evidence.measured("unknown managed collection action rejection");
      throw new IdmError(400, "Expecting String containing one of: patch triggerSyncCheck updateLastSync");
    }
    const name = op.request.query?._action;
    const script = typeof name === "string" ? own(type.actions, name) : undefined;
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
    const kinds = ["_queryId", "_queryExpression", "_queryFilter"].filter((key) => own(query, key) !== undefined);
    if (kinds.length !== 1) {
      this.evidence.measured("query requires exactly one query selector");
      throw new IdmError(400, "Only one of [_queryId, _queryExpression, _queryFilter] is supported; multiple detected");
    }
    if (query._queryExpression !== undefined) {
      this.evidence.measured("DS query expressions are unsupported");
      throw new IdmError(405, "Query Expressions are not supported when using DS as the repo");
    }
    if (query._queryId !== undefined) missing(this.evidence, `named _queryId: ${String(query._queryId)}`);
    // One grammar for both lanes (src/bindings/rhino/crest-filter.cjs): a catchable 400 only where
    // AIC's rejection is measured or certain; anything else it cannot decide is missing.
    const classified = classifyFilter(query._queryFilter);
    if ("error" in classified) {
      if (classified.error === "parse") {
        this.evidence.measured("CREST filter syntax and documented parse failures", "docs/api/10-managed-objects.md");
        throw new IdmError(400, "A value could not be parsed as a valid query filter");
      }
      if (classified.error === "ne" && !op.internal) {
        this.evidence.measured("ne is refused with an operator-specific 400 over REST", "docs/api/10-managed-objects.md");
        throw new IdmError(400, "unrecognized or unsupported filter operator 'ne'");
      }
      const reason = classified.error === "ne" ? "ne from a script (AIC refuses it; the script-side text is unmeasured)" : classified.reason;
      missing(this.evidence, `_queryFilter the emulator cannot decide: ${JSON.stringify(query._queryFilter) ?? "undefined"}: ${reason}`, "openidm-query-filter");
    }
    this.evidence.measured("CREST filter syntax and documented parse failures", "docs/api/10-managed-objects.md");
    this.evidence.assumed("CREST comparison uses existential array matching");
    const filter = classified.filter;
    checkCapabilities(type, false, this.evidence, this.policy !== undefined);
    const policy = stringPolicy(type.name, this.evidence, (field) => this.jsonStored(type.name, field));
    // AIC converts each assertion to its attribute's syntax before reading a record, so a literal
    // it cannot convert is a 400 on an empty collection too (docs/api/10, 2026-11-06).
    const illegal = illegalAssertion(filter, policy);
    if (illegal !== null) {
      this.evidence.measured("a literal a DS integer or time attribute cannot convert is a 400 illegal filter assertion value",
        "docs/api/10-managed-objects.md (E19 live probe, 2026-11-06)");
      throw new IdmError(400, `The request cannot be processed because it contained an illegal filter assertion value '${illegal.value}' for field '${illegal.field}'`,
        undefined, undefined, true);
    }
    let rows = [...this.records.entries()].filter(([key, row]) => key.startsWith(`managed/${type.name}/`) && matchesFilter(filter, this.queried(key, row), policy)).map(([, row]) => structuredClone(row));
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
      const parts = query._pagedResultsCookie.split(".");
      const [encoded, signature] = parts;
      if (parts.length !== 2 || !encoded || signature !== sign(encoded)) throw invalidInput("Invalid paging cookie");
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
        return this.projectVisible(type, row, selectors, op);
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

const QUERY_SOURCE = "docs/api/10-managed-objects.md (eq/gt/lt ignore case and edge spaces, 2026-11-04)";
// Probed 2026-11-04 with a case-swapped eq; every one folded (doc 10).
const CASE_PROBED = ["givenName", "sn", "description", "frIndexedString1", "frUnindexedString1"];
const ORDERING = ["gt", "ge", "lt", "le"];

/**
 * Letter case and edge spaces in string comparisons, from doc 10 (2026-11-04). Only `alpha_user`
 * was probed. Its probes span indexed, unindexed and `description` properties and all folded, which
 * is LDAP caseIgnoreMatch / caseIgnoreOrderingMatch on the DS repository; the unprobed rest of
 * `alpha_user` is assumed to share that rule. `co`/`sw` and every other managed type were not
 * probed, and either guess can be a false pass there (a folding guess passes a "not found" test,
 * an exact guess a `false`-branch test), so a record whose answer turns on them is missing.
 */
function stringPolicy(type: string, evidence: Evidence, jsonStored: (field: string) => boolean): StringPolicy {
  const name = (field: string) => field.replace(/^\//, "");
  return {
    rules(_field, op): Record<RuleDimension, StringRule> {
      if (type !== "alpha_user" || op === "co" || op === "sw") return { case: "unknown", space: "unknown" };
      return { case: "fold", space: "fold" };
    },
    // Round-5 P2-1: measured on alpha_user frIndexedInteger1, an LDAP integer attribute, so the
    // quoted text reaches DS as an integer assertion. Generic types and alpha_user custom_*
    // properties are JSON in DS (caseIgnoreJsonQueryMatch), where coercion is unmeasured.
    quotedInteger(field) {
      const f = name(field);
      if (type !== "alpha_user" || f.startsWith("custom_") || f.includes("/")) return false;
      if (f === "frIndexedInteger1") {
        evidence.measured("eq on alpha_user frIndexedInteger1 matches a quoted integer", "docs/api/10-managed-objects.md (A number matches quoted or unquoted)");
      } else {
        evidence.assumed(`eq on alpha_user ${f} matches a quoted integer like frIndexedInteger1`);
      }
      return true;
    },
    // Round-5 P3-5: nothing measured these beyond the unquoted frIndexedInteger1 eq.
    scalar(field, op, kind) {
      const f = name(field);
      if (kind === "number" && op === "eq" && type === "alpha_user" && f === "frIndexedInteger1") {
        evidence.measured("eq on alpha_user frIndexedInteger1 matches an unquoted integer", "docs/api/10-managed-objects.md (A number matches quoted or unquoted)");
      } else if (kind === "number") {
        evidence.assumed(`${op} on a number property compares JSON numbers numerically`);
      } else {
        evidence.assumed("eq on a boolean property compares JSON booleans");
      }
    },
    // telephoneNumber, the time and integer attributes, the multivalued `pr` rule
    // (crest-filter.cjs `fieldRule`), and a generic type's JSON-stored property.
    rule: (field) => fieldRule(type, name(field)) ?? (jsonStored(name(field)) ? "json" : null),
    label(provenance, behavior, source) {
      if (provenance === "measured") evidence.measured(behavior, source);
      else evidence.assumed(behavior);
    },
    used(field, op, dimension) {
      const f = name(field);
      if (dimension === "case") {
        if (op === "eq" && (f === "mail" || f === "userName")) {
          evidence.measured("eq on alpha_user mail and userName ignores letter case",
            "docs/api/10-managed-objects.md (eq on mail and userName is case-insensitive, 2026-11-04)");
        } else if (op === "eq" && CASE_PROBED.includes(f)) {
          evidence.measured(`eq on alpha_user ${CASE_PROBED.join(", ")} ignores letter case`, QUERY_SOURCE);
        } else if (f === "userName" && op !== "le") {
          evidence.measured("gt, ge and lt on alpha_user userName ignore letter case", QUERY_SOURCE);
        } else {
          evidence.assumed(`${op} on alpha_user ${f} ignores letter case: unprobed, assumed like the probed properties`);
        }
      } else if (op === "eq" && (f === "mail" || f === "userName")) {
        evidence.measured("eq on alpha_user mail and userName ignores leading and trailing spaces", QUERY_SOURCE);
      } else {
        evidence.assumed(`${op} on alpha_user ignores leading and trailing spaces like eq on mail and userName`);
      }
    },
    ordered() {
      // Only same-case letters were ordered on the tenant (userName gt "<p>-alicd"); punctuation
      // against folded letters and non-ASCII collation were not.
      evidence.assumed("string ordering compares UTF-16 code units after the case and space rules");
    },
    unknown(field, op, dimension): never {
      const what = {
        case: `${ORDERING.includes(op as Operator) ? "order" : "match"} turns on letter case`,
        space: "match turns on leading, trailing or repeated spaces or non-U+0020 whitespace",
        unicode: "match turns on Unicode normalisation or characters RFC 4518 maps to nothing",
        type: "match compares different JSON types, or an object or null element, which nothing measured",
        presence: "answer for an empty string, array or object is unmeasured",
        rule: "field's DS matching rule is not caseIgnoreMatch (telephone, time or integer), and the 2026-11-06 probe measured neither this field nor this operator and literal form",
        path: "pointer reads through an array, measured only as a bare eq reached through and alone",
      }[dimension];
      return missing(evidence, `query ${op} whose ${what}: ${type} ${field}`, "openidm-query-filter");
    },
  };
}
