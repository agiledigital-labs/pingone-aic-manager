/** Framework-free authenticate state machine. AM executes scripts; the supplied IDM instance owns storage. */
import { createHmac, randomBytes, randomUUID, timingSafeEqual } from "node:crypto";
import type { CallbackEffect } from "../case/types.ts";
import { EmulationFailure, EmulationFailureJournal, emulationFailure, responseFailure } from "../emulation.ts";
import type { IdmEmulator } from "../idm/emulator.ts";
import { isDocument } from "../idm/document.ts";
import { Evidence } from "../idm/provenance.ts";
import type { Document } from "../idm/types.ts";
import type { HostCall, JsonValue } from "../protocol.ts";
import { HostCallError, type RhinoRunner } from "../runner.ts";
import { decisionNodeClassAllowList } from "../bindings/allowlist.ts";
import { SUCCESS_NODE_ID, FAILURE_NODE_ID } from "../aic/constants.ts";
import { fromWire, toWire, type WireCallback } from "./callbacks.ts";
import { JourneyNodes, EMULATED, type NodeResult } from "./nodes.ts";
import { JourneyConfig } from "./config.ts";
import { journeyPreamble, JOURNEY_HARVEST } from "./script.ts";
import type { AuthenticateRequest, AuthenticateResult, HttpMock, NodeDocument, TreeDocument } from "./types.ts";
const SOURCE = "docs/api/25-journey-runtime.md (measurement agent, 2026-11-02)";
type Frame = { tree: string; node: string; transient: Document };
type State = { shared: Document; frames: Frame[]; errorMessage: string | null; pending?: WireCallback[]; page?: { counts: number[] }; existingSession?: Record<string, string> };
export type JourneyOptions = {
  /** Outcomes by node id, for otherwise unimplemented node types only. */
  declaredOutcomes?: Readonly<Record<string, string>>;
  http?: HttpMock;
  /** Complete existing-session snapshots supplied by cookie value; not fabricated from tokenId. */
  sessions?: Readonly<Record<string, Readonly<Record<string, string>>>>;
  cookieName?: string;
  maxTransitions?: number;
  timeoutMs?: number;
};
const OUTCOMES: Readonly<Record<string, readonly string[]>> = Object.freeze({
  UsernameCollectorNode: ["outcome"], PasswordCollectorNode: ["outcome"], AttributeCollectorNode: ["outcome"], ValidatedUsernameNode: ["outcome"], ValidatedPasswordNode: ["outcome"],
  IdentifyExistingUserNode: ["true", "false"], CreateObjectNode: ["CREATED", "FAILURE"], PatchObjectNode: ["PATCHED", "FAILURE"],
});
export class JourneyEmulator {
  readonly config: JourneyConfig;
  readonly idm: IdmEmulator;
  private readonly runner: RhinoRunner;
  private readonly options: JourneyOptions;
  private readonly evidence = new Evidence();
  private readonly nodes: JourneyNodes;
  private failures = new EmulationFailureJournal();
  private key = randomBytes(32);
  private snapshots = new Map<string, State>();
  private completed = new Set<string>();
  constructor(config: JourneyConfig, runner: RhinoRunner, idm: IdmEmulator, options: JourneyOptions = {}) {
    this.config = config; this.runner = runner; this.idm = idm;
    this.nodes = new JourneyNodes(idm, this.evidence, (name) => this.missing(name));
    for (const tree of Object.values(config.trees)) for (const [id, node] of Object.entries(tree.nodes)) {
      if (Object.hasOwn(options.declaredOutcomes ?? {}, id) && EMULATED.includes(node.nodeType)) throw new TypeError(`declared outcome for emulated node: ${node.nodeType}/${id}`);
    }
    for (const [type, configs] of Object.entries(config.nodes)) if (EMULATED.includes(type)) for (const id of Object.keys(configs)) {
      if (Object.hasOwn(options.declaredOutcomes ?? {}, id)) throw new TypeError(`declared outcome for emulated node: ${type}/${id}`);
    }
    this.options = { ...options, declaredOutcomes: Object.freeze(structuredClone(options.declaredOutcomes ?? {})), sessions: structuredClone(options.sessions ?? {}) };
    if (!Number.isSafeInteger(options.timeoutMs ?? 10000) || (options.timeoutMs ?? 10000) < 0) throw new TypeError("timeoutMs must be a nonnegative integer");
    if (!Number.isSafeInteger(options.maxTransitions ?? 1000) || (options.maxTransitions ?? 1000) < 1) throw new TypeError("maxTransitions must be a positive integer");
  }
  fork(): JourneyEmulator {
    const fork = new JourneyEmulator(this.config, this.runner, this.idm.fork(), this.options);
    fork.key = Buffer.from(this.key); fork.snapshots = structuredClone(this.snapshots); fork.completed = new Set(this.completed); fork.failures = this.failures;
    return fork;
  }
  get emulationFailures() { return this.failures.entries; }
  private missing(name: string): never {
    this.evidence.add("missing", name);
    throw new EmulationFailure({ kind: "missing", message: `Missing journey behavior: ${name}`, missing: [name] });
  }
  private measured(name: string): void { this.evidence.measured(name, SOURCE); }
  request(request: AuthenticateRequest): Promise<AuthenticateResult> {
    return this.failures.run(`authenticate ${request.query.authIndexValue}`, async () => {
      const { value, provenance } = await this.evidence.run(async () => {
        try { return await this.authenticate(request); }
        catch (error) {
          const fatal = emulationFailure(error) ?? { kind: "internal" as const, message: String(error) };
          for (const name of fatal.missing ?? []) this.evidence.add("missing", name);
          this.failures.record(fatal, `authenticate ${request.query.authIndexValue}`);
          return { status: fatal.kind === "missing" ? 501 : 500, body: { code: fatal.kind === "missing" ? 501 : 500, reason: "Emulation Failure", message: fatal.message }, emulation: fatal };
        }
      });
      return { ...value, provenance };
    });
  }
  private error(status: number, message: string): Omit<AuthenticateResult, "provenance"> {
    return { status, body: { code: status, reason: status === 400 ? "Bad Request" : "Unauthorized", message } };
  }
  private empty(): Omit<AuthenticateResult, "provenance"> { return { status: 500, body: null, rawBody: "" }; }
  private snapshot(state: State): string {
    const id = randomUUID(); const stored = structuredClone(state);
    for (const frame of stored.frames) frame.transient = {};
    this.snapshots.set(id, stored);
    if (state.errorMessage !== null) this.evidence.assumed("action error message persists across node and callback boundaries");
    this.evidence.assumed("authId uses a random snapshot id and HMAC-SHA256; forks copy snapshots and share signing credentials");
    return `${id}.${createHmac("sha256", this.key).update(id).digest("base64url")}`;
  }
  private restore(token: string): State | undefined {
    const [id, mac, extra] = token.split(".");
    if (!id || !mac || extra !== undefined || !/^[A-Za-z0-9_-]{43}$/.test(mac)) return undefined;
    const expected = createHmac("sha256", this.key).update(id).digest("base64url");
    if (mac.length !== expected.length || !timingSafeEqual(Buffer.from(mac), Buffer.from(expected))) return undefined;
    const snapshot = this.snapshots.get(id);
    return snapshot ? structuredClone(snapshot) : undefined;
  }
  private tree(name: string): TreeDocument {
    const tree = this.config.tree(name);
    if (!tree) this.missing(`inner tree configuration: ${name}`);
    for (const flag of ["noSession", "transactionalOnly", "mustRun"]) if (tree[flag]) this.missing(`tree ${flag}`);
    for (const flag of ["maximumIdleTime", "maximumSessionTime", "treeTimeout", "staticNodes"]) if (Object.hasOwn(tree, flag)) this.missing(`tree ${flag}`);
    if (tree.enabled === false) this.missing("disabled tree behavior");
    return tree;
  }
  private async authenticate(request: AuthenticateRequest): Promise<Omit<AuthenticateResult, "provenance">> {
    if (request.method !== "POST" || !["authenticate", "/authenticate", `/am/json/realms/root/realms/${this.config.realm}/authenticate`].includes(request.path)) this.missing(`journey REST route: ${request.method} ${request.path}`);
    if (request.query.authIndexType !== "service") this.missing(`authIndexType: ${request.query.authIndexType}`);
    let body: JsonValue = request.body ?? {};
    if (request.rawBody !== undefined) {
      try { body = JSON.parse(request.rawBody) as JsonValue; }
      catch { this.missing("authenticate invalid JSON body"); }
    }
    if (!isDocument(body)) this.missing("authenticate request body shape");
    const name = request.query.authIndexValue;
    const tree = this.config.tree(name);
    this.measured("unknown and innerTreeOnly direct journeys return No Configuration found");
    if (!tree || tree.innerTreeOnly) return this.error(400, "No Configuration found");
    let state: State;
    if (Object.hasOwn(body, "authId")) {
      if (typeof body.authId !== "string") return this.error(400, "Authentication Error: Invalid AuthId Signature");
      const restored = this.restore(body.authId);
      this.measured("corrupted authId returns Invalid AuthId Signature");
      if (!restored || restored.frames[0]?.tree !== name) return this.error(400, "Authentication Error: Invalid AuthId Signature");
      if (this.completed.has(body.authId)) this.missing("authId replay after session success");
      state = restored;
      this.measured("stale callback authId replay is accepted; shared state survives and transient is dropped");
      this.evidence.assumed("authId uses a random snapshot id and HMAC-SHA256; forks copy snapshots and share signing credentials");
    } else {
      state = { shared: { realm: `/${this.config.realm}`, authLevel: 0 }, frames: [{ tree: name, node: tree.entryNodeId, transient: { maxAuthenticationSessionDuration: 300000 } }], errorMessage: null };
      this.evidence.assumed("initial ambient state assumes shared realm/authLevel 0 and transient maxAuthenticationSessionDuration 300000; duration value and bucket are unmeasured");
      const cookieName = this.options.cookieName;
      if (Object.keys(request.cookies ?? {}).length && !cookieName) this.missing("session cookie name not supplied");
      const cookie = cookieName ? request.cookies?.[cookieName] : undefined;
      if (cookie) {
        const sessions = this.options.sessions;
        if (!sessions || !Object.hasOwn(sessions, cookie)) this.missing("existingSession cookie not supplied in session fixtures");
        state.existingSession = structuredClone(sessions[cookie]) as Record<string, string>;
        this.evidence.assumed("existingSession is the caller-supplied complete cookie session snapshot");
      }
      if (body.callbacks !== undefined) this.measured("callbacks without authId start a fresh journey and reach its first node");
    }
    let submitted = fromWire(body.callbacks, (name) => this.missing(name));
    if (state.pending) {
      const raw = body.callbacks;
      if (!Array.isArray(raw) || raw.length !== state.pending.length) this.missing("authenticate callback submission differs from snapshot");
      for (const [index, expected] of state.pending.entries()) {
        const actual = raw[index];
        if (!isDocument(actual) || actual.type !== expected.type || JSON.stringify(actual.output) !== JSON.stringify(expected.output)) this.missing("authenticate callback submission differs from snapshot");
        const names = Array.isArray(actual.input) ? actual.input.map((item) => isDocument(item) ? item.name : null) : [];
        if (JSON.stringify(names) !== JSON.stringify((expected.input ?? []).map((item) => item.name))) this.missing("authenticate callback submission differs from snapshot");
      }
      delete state.pending;
    }
    for (let step = 0; step < (this.options.maxTransitions ?? 1000); step++) {
      const frame = state.frames.at(-1);
      if (!frame) throw new Error("journey frame invariant");
      const currentTree = this.tree(frame.tree);
      const inner = state.frames.length > 1;
      if (frame.node === SUCCESS_NODE_ID || frame.node === FAILURE_NODE_ID) {
        const success = frame.node === SUCCESS_NODE_ID;
        if (inner) {
          state.frames.pop(); const parent = state.frames.at(-1);
          if (!parent) throw new Error("inner frame invariant");
          this.measured("inner success/failure maps to true/false and restores outer transient state");
          const reply = this.advance(state, success ? "true" : "false");
          if (reply) return reply;
          continue;
        }
        this.measured("failure is 401 with action error message; success requires shared username or returns empty 500");
        if (!success) return this.error(401, state.errorMessage ?? "Login failure");
        if (!state.shared.username) return this.empty();
        this.evidence.assumed("session tokenId is a local random UUID; principal existence and session properties are not modeled");
        if (this.config.realm !== "alpha") this.missing(`session success realm: ${this.config.realm}`);
        if (typeof body.authId === "string") this.completed.add(body.authId);
        return { status: 200, body: { tokenId: randomUUID(), successUrl: "/enduser/?realm=/alpha", realm: "/alpha" } };
      }
      const node = currentTree.nodes[frame.node];
      if (!node) this.missing(`tree node reference: ${frame.tree}/${frame.node}`);
      const config = this.config.node(node.nodeType, frame.node);
      if (!config) this.missing(`node configuration: ${node.nodeType}/${frame.node}`);
      if (node.nodeType === "InnerTreeEvaluatorNode") {
        if (inner) this.missing("nested inner trees");
        if (typeof config.tree !== "string") this.missing("inner tree target configuration");
        const innerTree = this.tree(config.tree);
        this.measured("inner trees share shared state and use isolated transient state; journey names follow the executing tree");
        state.frames.push({ tree: config.tree, node: innerTree.entryNodeId, transient: {} });
        submitted = []; continue;
      }
      const result = await this.execute(node.nodeType, frame.node, config, state, frame, submitted, request);
      if (result.abort) return result.abort;
      if (result.callbacks.length) {
        if (inner) this.missing("inner tree callbacks");
        this.measured("queued callbacks supersede goTo; node re-executes on resume with shared state preserved");
        const callbacks = toWire(result.callbacks, (name) => this.missing(name));
        state.pending = callbacks;
        return { status: 200, body: { authId: this.snapshot(state), callbacks } };
      }
      const outcome = result.outcome;
      const allowed = this.outcomes(node.nodeType, config);
      if (outcome === null || (allowed !== undefined && !allowed.includes(outcome))) {
        this.measured("undeclared node outcome returns 401 Login failure");
        return this.error(401, "Login failure");
      }
      const reply = this.advance(state, outcome); if (reply) return reply;
      submitted = [];
    }
    throw new EmulationFailure({ kind: "protocol", message: "journey transition limit exceeded" });
  }
  private async scripted(config: NodeDocument, state: State, frame: Frame, submitted: CallbackEffect[], request: AuthenticateRequest): Promise<NodeResult> {
    const currentTree = this.tree(frame.tree), inner = state.frames.length > 1;
        for (const key of ["inputs", "outputs"]) {
          const selectors = config[key];
          if (selectors !== undefined && (!Array.isArray(selectors) || selectors.length !== 1 || selectors[0] !== "*")) this.missing(`ScriptedDecisionNode ${key} filtering`);
        }
        const script = typeof config.script === "string" ? this.config.script(config.script) : undefined;
        if (!script) this.missing(`script configuration: ${String(config.script)}`);
        if (script.evaluatorVersion !== "2.0") this.missing(`script evaluatorVersion: ${script.evaluatorVersion}`);
        if (script.language && script.language !== "JAVASCRIPT") this.missing(`script language: ${script.language}`);
        if (typeof script.script !== "string" || Buffer.from(script.script, "base64").toString("base64") !== script.script) this.missing("script base64 encoding");
        this.evidence.assumed("AM binding facsimiles reuse rhino-local's measured overlay; Java collection wrappers remain local approximations");
        this.measured("next-gen nodeState has no getShared/getTransient; callback-free boundaries retain transient state");
        this.measured("action withErrorMessage works in either call order; withLockoutMessage has no visible failure-message effect");
        const response = await this.runner.eval({ engine: "am", hostCalls: true, timeoutMs: this.options.timeoutMs ?? 10000, source: Buffer.from(script.script, "base64").toString("utf8") + JOURNEY_HARVEST,
          sourceName: script.name ?? config.script ?? "script", classAllowList: decisionNodeClassAllowList(),
          preamble: journeyPreamble({ shared: state.shared, transient: frame.transient, callbacks: submitted, tree: frame.tree, inner, identityResource: currentTree.identityResource ?? `managed/${this.config.realm}_user`, realm: this.config.realm, scriptName: script.name ?? config.script ?? "script", scriptId: config.script ?? "script", ...(state.existingSession ? { existingSession: state.existingSession } : {}), ...(request.headers ? { headers: { ...request.headers } } : {}), ...(request.cookies ? { cookies: { ...request.cookies } } : {}), parameters: Object.fromEntries(Object.entries(request.query).map(([key, value]) => [key, typeof value === "string" ? [value] : [...value]])) }),
        }, { onCall: this.evidence.bind((call) => this.host(call)) });
        for (const entry of response.provenance ?? []) this.evidence.add(entry.kind, entry.behavior, entry.source);
        const fatal = responseFailure(response); if (fatal) throw new EmulationFailure(fatal);
        if (response.outcome === "compile_error") this.missing("journey script compilation failure");
        if (response.outcome !== "ok") {
          if (inner) this.missing("inner tree script exception");
          this.measured("thrown scripted decision error returns 401 Login failure");
          return { outcome: null, callbacks: [], abort: this.error(401, "Login failure") };
        }
        if (typeof response.value !== "string") this.missing("journey harvest serialization");
        const result = JSON.parse(response.value) as { effects: { sharedState: { final: Document }; transientState: { final: Document }; callbacks: CallbackEffect[]; outcome: string | null }; errorMessage: string | null };
        state.shared = result.effects.sharedState.final; frame.transient = result.effects.transientState.final;
        if (result.errorMessage !== null) state.errorMessage = result.errorMessage;
    return { outcome: result.effects.outcome, callbacks: result.effects.callbacks };
  }
  private async execute(type: string, id: string, config: NodeDocument, state: State, frame: Frame, submitted: CallbackEffect[], request: AuthenticateRequest): Promise<NodeResult> {
    if (type === "ScriptedDecisionNode") return this.scripted(config, state, frame, submitted, request);
    if (type === "PageNode") return this.page(config, state, frame, submitted, request);
    if (EMULATED.includes(type)) return this.nodes.run(type, config, state.shared, frame.transient, submitted, this.tree(frame.tree).identityResource ?? `managed/${this.config.realm}_user`);
    const declarations = this.options.declaredOutcomes;
    if (!declarations || !Object.hasOwn(declarations, id)) this.missing(`node type: ${type}`);
    const outcome = declarations[id] ?? null;
    this.evidence.assumed(`declared by test: ${frame.tree}/${id} -> ${String(outcome)}`);
    return { outcome, callbacks: [] };
  }
  private async page(config: NodeDocument, state: State, frame: Frame, submitted: CallbackEffect[], request: AuthenticateRequest): Promise<NodeResult> {
    if (state.frames.length > 1) this.missing("inner tree callbacks");
    for (const field of ["pageHeader", "pageDescription"]) if (isDocument(config[field]) ? Object.keys(config[field]).length : config[field] !== undefined) this.missing(`PageNode ${field}`);
    if (!Array.isArray(config.nodes) || !config.nodes.length) this.missing("PageNode child configuration");
    const previous = state.page;
    delete state.page;
    if (!previous && submitted.length) this.missing("PageNode callbacks without authId");
    const counts: number[] = [], collected: CallbackEffect[] = [];
    let offset = 0, outcome: string | null = null;
    for (const [index, child] of config.nodes.entries()) {
      if (!isDocument(child) || typeof child._id !== "string" || typeof child.nodeType !== "string") this.missing("PageNode child reference");
      if (child.nodeType === "PageNode") this.missing("nested PageNode");
      const childConfig = this.config.node(child.nodeType, child._id);
      if (!childConfig) this.missing(`PageNode child configuration: ${child._id}`);
      const count = previous?.counts[index] ?? 0;
      const own = previous ? submitted.slice(offset, offset + count) : [];
      offset += count;
      const result = await this.execute(child.nodeType, child._id, childConfig, state, frame, own, request);
      if (result.abort) return result;
      counts.push(result.callbacks.length);
      collected.push(...result.callbacks);
      outcome = result.outcome;
    }
    this.measured("PageNode children run in order with only their own callbacks; last child supplies the outcome");
    if (!previous) {
      state.page = { counts };
      state.shared.pageNodeCallbacks = toWire(collected, (name) => this.missing(name)) as JsonValue;
      this.evidence.assumed("pageNodeCallbacks stores the merged callback wire array; only shared presence and lifetime are measured");
      this.evidence.assumed("PageNode callback discovery executes scripts once and retains their state effects");
      return { outcome: null, callbacks: collected };
    }
    if (collected.length) {
      // A rejected collector re-sends the entire page, including siblings' answers.
      const retry = submitted.map((cb) => ({ ...cb }));
      let start = 0;
      for (const [index, count] of previous.counts.entries()) {
        if (counts[index]) {
          if (counts[index] !== count) this.missing("PageNode callback count changes on resubmission");
          retry.splice(start, count, ...collected.splice(0, count));
        }
        start += count;
      }
      state.page = previous;
      state.shared.pageNodeCallbacks = toWire(retry, (name) => this.missing(name)) as JsonValue;
      this.evidence.assumed("pageNodeCallbacks stores the merged callback wire array; only shared presence and lifetime are measured");
      this.measured("required empty attribute re-sends the whole page with echoed values and empty failedPolicies");
      return { outcome: null, callbacks: retry };
    }
    return { outcome, callbacks: [] };
  }
  private outcomes(type: string, config: NodeDocument): readonly string[] | undefined {
    if (type === "ScriptedDecisionNode") return config.outcomes ?? [];
    if (type === "InnerTreeEvaluatorNode") return config.displayErrorOutcome ? ["true", "false", "error"] : ["true", "false"];
    if (type === "PageNode") {
      if (!Array.isArray(config.nodes) || config.nodes.length === 0) return [];
      const last = config.nodes.at(-1);
      if (!isDocument(last) || typeof last._id !== "string" || typeof last.nodeType !== "string") this.missing("PageNode declared outcome child reference");
      if (last.nodeType === "PageNode") this.missing("nested PageNode outcome declaration");
      const child = this.config.node(last.nodeType, last._id);
      if (!child) this.missing(`PageNode declared outcome child configuration: ${last._id}`);
      return this.outcomes(last.nodeType, child);
    }
    return Object.hasOwn(OUTCOMES, type) ? OUTCOMES[type] : undefined;
  }
  private advance(state: State, outcome: string): Omit<AuthenticateResult, "provenance"> | undefined {
    const frame = state.frames.at(-1); if (!frame) throw new Error("journey advance frame invariant");
    const node = this.tree(frame.tree).nodes[frame.node]; if (!node) throw new Error("journey advance node invariant");
    if (!Object.hasOwn(node.connections, outcome)) { this.measured("declared but unwired node outcome returns empty 500"); return this.empty(); }
    if (state.errorMessage !== null) this.evidence.assumed("action error message persists across node and callback boundaries");
    frame.node = node.connections[outcome] ?? "";
    return undefined;
  }
  private checkHostValue(value: JsonValue): void {
    try {
      JSON.stringify(value, (_key, item: unknown) => {
        if (typeof item === "number" && (!Number.isFinite(item) || Object.is(item, -0))) this.missing("AM host non-finite or negative-zero result");
        if (["undefined", "function", "symbol", "bigint"].includes(typeof item)) this.missing("AM host non-JSON result");
        return item;
      });
    } catch (error) {
      if (emulationFailure(error)) throw error;
      throw new EmulationFailure({ kind: "protocol", message: "AM host result is not JSON serializable" });
    }
  }

  private async host(call: HostCall): Promise<JsonValue> {
    if (call.method !== "http") {
      try { const result = await this.idm.onCall(call); this.checkHostValue(result); return result; }
      catch (error) {
        if (error instanceof HostCallError) this.evidence.assumed("AM openidm errors use a JavaException-named JS adapter with string code and JSON detail");
        throw error;
      }
    }
    const [url, options] = call.args;
    if (typeof url !== "string" || !isDocument(options)) this.missing("httpClient request shape");
    if (!this.options.http) this.missing(`unmocked httpClient.send: ${url}`);
    this.evidence.assumed("httpClient response is supplied by the test mock; get/json/text wrappers reuse AM binding shapes");
    let response: Awaited<ReturnType<HttpMock>>;
    try { response = await this.options.http(url, options); }
    catch (error) { throw new EmulationFailure(emulationFailure(error) ?? { kind: "internal", message: `journey HTTP mock failed: ${String(error)}` }); }
    if (!response || !Number.isInteger(response.status) || response.status < 100 || response.status > 599) this.missing(`httpClient mock response: ${url}`);
    const result = { status: response.status, body: response.body ?? null, headers: response.headers ?? {} };
    this.checkHostValue(result);
    return result;
  }
}
