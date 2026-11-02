import { EmulationFailure, EmulationFailureJournal, emulationFailure } from "../emulation.ts";
/** REST seam and script-facing host adapter. State belongs to an instance; config is shared. */
import { AsyncLocalStorage } from "node:async_hooks";
import type { EmulationFailureInfo, HostCall, JsonValue } from "../protocol.ts";
import { HostCallError, type RhinoRunner } from "../runner.ts";
import { IdmConfig } from "./config.ts";
import { validateDeclarations } from "./declarations.ts";
import { document, isDocument, own } from "./document.ts";
import { endpoint } from "./endpoints.ts";
import { escapeError, IdmError, invalidInput, missing } from "./errors.ts";
import { ManagedStore, type Operation, type Reply } from "./managed.ts";
import { Evidence } from "./provenance.ts";
import { Scripts } from "./scripts.ts";
import type { Document, PolicyEvaluator, RestRequest, RestResult } from "./types.ts";

export class IdmEmulator {
  readonly config: IdmConfig;
  private readonly runner: RhinoRunner;
  private readonly properties: Record<string, JsonValue>;
  private readonly policy: PolicyEvaluator | undefined;
  private readonly records: Map<string, Document>;
  private readonly evidence = new Evidence();
  private failures = new EmulationFailureJournal();
  private readonly chain = new AsyncLocalStorage<Document>();
  private readonly scripts: Scripts;
  private readonly managed: ManagedStore;

  constructor(config: IdmConfig, runner: RhinoRunner, options: { policy?: PolicyEvaluator; properties?: Record<string, JsonValue> } = {}) {
    this.config = config;
    this.runner = runner;
    this.policy = options.policy;
    this.properties = structuredClone(options.properties ?? {});
    this.records = new Map();
    this.scripts = new Scripts(runner, this.evidence, (call, context) => this.chain.run(context, () => this.host(call)), this.properties);
    this.managed = new ManagedStore(this.records, this.scripts, this.evidence, this.policy);
  }

  /** Shares frozen config and runner; copies all records, including nested JSON values. */
  fork(): IdmEmulator {
    const fork = new IdmEmulator(this.config, this.runner, { ...(this.policy ? { policy: this.policy } : {}), properties: this.properties });
    for (const [key, value] of this.records) fork.records.set(key, structuredClone(value));
    fork.managed.inheritCookies(this.managed);
    fork.failures = this.failures;
    return fork;
  }

  /** Forks share the audit view; verdict ownership belongs to each operation. */
  get emulationFailures() { return this.failures.entries; }

  private failure(fatal: EmulationFailureInfo): Omit<RestResult, "provenance"> {
    for (const name of fatal.missing ?? []) this.evidence.add("missing", name);
    const status = fatal.kind === "missing" ? 501 : 500;
    return { status, body: new IdmError(status, fatal.message).body, emulation: fatal };
  }

  request(request: RestRequest): Promise<RestResult> {
    return this.failures.run(`request ${request.method} ${request.path}`, () => this.requestOwned(request));
  }

  private async requestOwned(request: RestRequest): Promise<RestResult> {
    const { value, provenance } = await this.evidence.run<Omit<RestResult, "provenance">>(async () => {
      try {
        validateDeclarations(this.config, this.evidence);
        const headers: Record<string, string> = Object.create(null) as Record<string, string>;
        for (const [name, value] of Object.entries(request.headers ?? {})) {
          const existing = Object.keys(headers).find((key) => key.toLowerCase() === name.toLowerCase());
          headers[existing ?? name] = existing ? `${headers[existing]},${value}` : value;
        }
        const context: Document = { http: { method: request.method, path: request.path,
          headers, parameters: { ...request.query } } };
        this.evidence.assumed("context contains originating HTTP fields; authentication is not modeled");
        this.evidence.measured("HTTP header lookup is case insensitive with original key spelling", "docs/api/11-idm-endpoints.md");
        return await this.chain.run(context, () => this.route({ request, context, internal: false }));
      } catch (error) {
        const fatal = emulationFailure(error);
        if (fatal) return this.failure(fatal);
        if (!(error instanceof IdmError)) return this.failure({ kind: "internal", message: String(error) });
        if (error.assumption) this.evidence.assumed(error.assumption);
        if (error.escapeHttp) this.evidence.measured("endpoint HTTP error strings are HTML escaped recursively", "docs/api/11-idm-endpoints.md");
        return { status: error.status, body: error.rawBody === undefined ? error.escapeHttp ? escapeError(error.body) : error.body : null,
          ...(error.rawBody === undefined ? {} : { rawBody: error.rawBody }) };
      }
    });
    // A script may catch a host exception, but cannot turn missing emulation into success.
    const absent = provenance.find((entry) => entry.kind === "missing");
    if (absent && !value.emulation) {
      const missing = provenance.filter((entry) => entry.kind === "missing").map((entry) => entry.behavior);
      const fatal: EmulationFailureInfo = { kind: "missing", message: `Missing IDM behavior: ${missing.join(", ")}`, missing };
      const failure = new IdmError(501, fatal.message);
      this.failures.record(fatal, `${request.method} ${request.path}`);
      return { status: failure.status, body: failure.body, emulation: fatal, provenance };
    }
    if (value.emulation) this.failures.record(value.emulation, `${request.method} ${request.path}`);
    return { ...value, provenance };
  }

  /** Attach directly to runner.eval(..., { onCall: emulator.onCall }).
   * REST calls collect nested provenance on their result. Independent script
   * hosts can use hostRequest to inspect REST provenance directly. Host evidence
   * also travels on replies into the evaluation report. Missing evidence travels
   * in the host error metadata and is retained by the outer JVM job even if caught. */
  readonly onCall = async (call: HostCall): Promise<JsonValue> => {
    const result = await this.hostRequest(call);
    if (result.emulation) throw new EmulationFailure(result.emulation);
    const absent = result.provenance.filter((entry) => entry.kind === "missing").map((entry) => entry.behavior);
    if (result.status >= 400 || absent.length > 0) {
      const body = document(result.body);
      const error = new IdmError(absent.length > 0 ? 501 : result.status, String(body.message), body.detail).host();
      throw new HostCallError({ class: error.class, code: error.code, message: error.message, detail: error.detail,
        ...(absent.length > 0 ? { missing: absent } : {}) });
    }
    return result.body;
  };

  hostRequest(call: HostCall): Promise<RestResult> {
    return this.failures.run(`host ${call.method} ${String(call.args[0])}`, () => this.hostRequestOwned(call));
  }

  private async hostRequestOwned(call: HostCall): Promise<RestResult> {
    const { value, provenance } = await this.evidence.run<Omit<RestResult, "provenance">>(async () => {
      try { return { status: 200, body: await this.host(call) }; }
      catch (error) {
        const fatal = emulationFailure(error);
        if (fatal) return this.failure(fatal);
        if (error instanceof IdmError) return { status: error.status, body: error.body };
        // host() projects errors for the JVM. Recover that shape for this REST result.
        if (isDocument(error) && typeof error.code === "number" && typeof error.message === "string") {
          return { status: error.code, body: new IdmError(error.code, error.message, error.detail).body };
        }
        return this.failure({ kind: "internal", message: String(error) });
      }
    });
    if ("emulation" in value && value.emulation) this.failures.record(value.emulation, `host ${call.method} ${String(call.args[0])}`);
    return { ...value, provenance };
  }

  private async route(op: Operation, forcedMethod?: string): Promise<Reply> {
    validateDeclarations(this.config, this.evidence);
    const path = op.request.path.replace(/^\/?(?:openidm\/)?/, "").replace(/\/$/, "");
    const parts = path.split("/");
    const recognized = new Set(["_action", "_fields", "_queryFilter", "_queryId", "_queryExpression",
      "_pageSize", "_pagedResultsOffset", "_pagedResultsCookie", "_sortKeys"]);
    for (const key of Object.keys(op.request.query ?? {})) {
      if (key.startsWith("_") && !recognized.has(key)) missing(this.evidence, `request parameter: ${key}`);
    }
    if (parts[0] === "config") {
      if (op.request.method === "GET" && parts[1] === "endpoint" && parts.length === 3 && this.config.endpointsSupplied && !own(this.config.endpoints, parts[2] ?? "")) {
        this.evidence.assumed("configured endpoint map also establishes config/endpoint snapshot absence");
        return this.knownAbsent(path, path, op, forcedMethod);
      }
      if (op.request.method !== "GET") missing(this.evidence, `config writes (config is read-only): ${path}`);
      if (Object.keys(op.request.query ?? {}).length > 0) missing(this.evidence, "config read query parameters");
      this.evidence.assumed("config snapshot reads are immediately consistent");
      if (path === "config/managed") return { status: 200, body: structuredClone(this.config.managed) as unknown as JsonValue };
      if (parts[1] === "endpoint" && parts.length === 3) {
        const config = own(this.config.endpoints, parts[2] ?? "");
        if (config) return { status: 200, body: structuredClone(config) as unknown as JsonValue };
      }
      missing(this.evidence, `resource route: ${path}`);
    }
    if (parts[0] === "managed") {
      const type = this.config.managedType(parts[1] ?? "");
      if (!type) return this.knownAbsent(path, parts.slice(0, 2).join("/"), op, forcedMethod);
      if (parts.length > 3) missing(this.evidence, "managed subresource routing");
      return this.managed.dispatch(type, parts[2], op);
    }
    if (parts[0] === "endpoint") {
      const config = own(this.config.endpoints, parts[1] ?? "");
      if (!config) {
        if (!this.config.endpointsSupplied) missing(this.evidence, `resource route: ${path}`);
        return this.knownAbsent(path, parts.slice(0, 2).join("/"), op, forcedMethod);
      }
      return endpoint(config, parts.slice(2).join("/"), op, this.scripts, this.evidence, forcedMethod);
    }
    missing(this.evidence, `resource route: ${path}`);
  }

  private knownAbsent(path: string, resource: string, op: Operation, hostMethod?: string): never {
    if (op.request.method !== "GET") missing(this.evidence, `resource route: ${path}`);
    if (hostMethod === "read") this.evidence.measured("unconfigured managed/endpoint names read as null internally", "orchestrator p24, 2026-11-02");
    if (hostMethod === "query") {
      if (resource.startsWith("managed/")) this.evidence.measured("unconfigured managed query throws NotFoundException", "orchestrator p24, 2026-11-02");
      else this.evidence.assumed("unconfigured endpoint query uses the managed NotFoundException diagnostic");
    }
    if (!op.internal) this.evidence.assumed("authoritative namespace absence uses REST 404");
    throw new IdmError(404, `Resource '${resource}' not found`);
  }

  private async host(call: HostCall): Promise<JsonValue> {
    try {
      validateDeclarations(this.config, this.evidence);
      const args = call.args;
      const resource = args[0];
      if (typeof resource !== "string") throw invalidInput("Expected resource name");
      let request: RestRequest;
      const queryAt = (index: number): Document => args[index] === null || args[index] === undefined ? {} : document(args[index]);
      const revisionHeaders = (): Record<string, string> => typeof args[1] === "string" ? { "If-Match": args[1] } : { "If-Match": "*" };
      switch (call.method) {
        case "read": {
          const query = queryAt(1);
          if (args[2] !== undefined) query._fields = args[2];
          request = { method: "GET", path: resource, query };
          break;
        }
        case "query": {
          const query = queryAt(1);
          if (args[2] !== undefined) {
            if (!Array.isArray(args[2])) missing(this.evidence, "query separate non-array fields argument");
            this.evidence.measured("separate fields array on IDM query is rejected");
            throw new IdmError(400, `The value '[${Array.isArray(args[2]) ? args[2].join(", ") : String(args[2])}]' for additional parameter '_fields' is not of expected type String`);
          }
          request = { method: "GET", path: resource, query };
          break;
        }
        case "create": {
          if (args[4] !== undefined) missing(this.evidence, "fifth create host argument");
          const body = document(args[2]);
          request = typeof args[1] === "string" ?
            { method: "PUT", path: `${resource.replace(/\/$/, "")}/${args[1]}`, headers: { "If-None-Match": "*" }, query: queryAt(3), body } :
            { method: "POST", path: resource, query: { ...queryAt(3), _action: "create" }, body };
          break;
        }
        case "update":
        case "patch":
          request = { method: call.method === "update" ? "PUT" : "PATCH", path: resource,
            query: queryAt(3), headers: revisionHeaders(), body: args[2] ?? null };
          break;
        case "delete":
          request = { method: "DELETE", path: resource, query: queryAt(2), headers: revisionHeaders() };
          break;
        case "action":
          if (args[4] !== undefined) missing(this.evidence, "fifth action host argument");
          request = { method: "POST", path: resource, query: { ...queryAt(3), _action: args[1] ?? null }, body: args[2] ?? null };
          break;
      }
      const context = this.chain.getStore() ?? {};
      if (call.method !== "read" && call.method !== "query") this.evidence.assumed("nested host writes commit independently");
      const result = await this.route({ request, context, internal: true }, call.method);
      if (result.rawBody !== undefined) missing(this.evidence, "script host consumption of invalid JSON endpoint output");
      return result.body;
    } catch (error) {
      if (!(error instanceof IdmError)) throw error;
      if (error.assumption) this.evidence.assumed(error.assumption);
      if (call.method === "read" && error.status === 404) {
        this.evidence.measured("openidm.read translates authoritative absence to null");
        return null;
      }
      throw error.host();
    }
  }
}
