/** Framework-free authenticate state machine. AM executes scripts; the supplied IDM instance owns storage. */
import { createHmac, randomBytes, randomUUID, timingSafeEqual } from "node:crypto";
import type { CallbackEffect } from "../case/types.ts";
import {
  EmulationFailure,
  EmulationFailureJournal,
  emulationFailure,
  responseFailure,
} from "../emulation.ts";
import type { IdmEmulator } from "../idm/emulator.ts";
import { isDocument } from "../idm/document.ts";
import { Evidence } from "../idm/provenance.ts";
import type { Document, Provenance } from "../idm/types.ts";
import type { HostCall, JsonValue } from "../protocol.ts";
import { HostCallError, type RhinoRunner } from "../runner.ts";
import { decisionNodeClassAllowList } from "../bindings/allowlist.ts";
import { SUCCESS_NODE_ID, FAILURE_NODE_ID } from "../aic/constants.ts";
import { fromWire, toWire, type WireCallback } from "./callbacks.ts";
import { JourneyNodes, EMULATED, type NodeResult } from "./nodes.ts";
import { JourneyConfig } from "./config.ts";
import { journeyPreamble } from "./script.ts";
import { JourneyLogs, type JourneyLogOptions, type RequestLog } from "./emit.ts";
import {
  EXTRA_OUTCOMES,
  STUB_TYPES,
  customNodeSource,
  customOutcomes,
  extraOutcomes,
} from "./extra-nodes.ts";
import type {
  AuthenticateRequest,
  AuthenticateResult,
  HttpMock,
  NodeDocument,
  ScriptDocument,
  PasswordPolicy,
  AttributePolicy,
  SessionDelta,
  TermsAndConditions,
  TreeDocument,
} from "./types.ts";
const SOURCE = "docs/api/25-journey-runtime.md (measurement agent, 2026-11-02)";
const HTTP_BODY_SOURCE =
  "docs/api/12-script-bindings-matrix.md (httpClient.send body serialization; §4 string escaping, 2026-11-04)";
type Frame = {
  tree: string;
  node: string;
  transient: Document;
  /** transient.objectAttributes is a Java map in AM, not a JS object. */
  javaMap?: boolean;
};
type State = {
  shared: Document;
  /** shared.objectAttributes is a Java map in AM, not a JS object. */
  javaMap?: boolean;
  frames: Frame[];
  errorMessage: string | null;
  pending?: WireCallback[];
  /** Callbacks each Page child sent at discovery, and the callbacks themselves. */
  page?: { counts: number[]; effects: CallbackEffect[] };
  existingSession?: Record<string, string>;
  /** What nodes have asked of the session the journey creates on success. */
  session?: SessionDelta;
  /** Evidence gathered before a round trip; the eventual result depends on it too. */
  provenance?: Provenance[];
  /** The journey's log identity (`trackingIds[0]`), kept across every round trip of it. */
  tracking?: string;
};
export type JourneyOptions = {
  /** Outcomes by node id, for otherwise unimplemented node types only. */
  declaredOutcomes?: Readonly<Record<string, string>>;
  http?: HttpMock;
  /** Complete existing-session snapshots supplied by cookie value; not fabricated from tokenId. */
  sessions?: Readonly<Record<string, Readonly<Record<string, string>>>>;
  cookieName?: string;
  /** The realm's Terms and Conditions, which Accept Terms and Conditions presents. */
  termsAndConditions?: TermsAndConditions;
  /** The realm password policy Validated Password evaluates when `validateInput` is true. */
  passwordPolicy?: PasswordPolicy;
  /** IDM policy per attribute name, which Attribute Collector evaluates when `validateInputs` is true. */
  attributePolicies?: Readonly<Record<string, AttributePolicy>>;
  maxTransitions?: number;
  timeoutMs?: number;
  /**
   * Where the AIC log records of each authenticate request go. Without a sink nothing is
   * emitted and nothing else changes. `origin` and `latencyMs` are the knobs of
   * src/journey/emit.ts.
   */
  logSink?: JourneyLogOptions["sink"];
  /**
   * The one clock, in epoch nanoseconds, injected so a virtual clock can drive it: log records
   * are stamped with it and nodes that stamp time (OTP, Terms and Conditions) read it. Defaults
   * to real time.
   */
  now?: JourneyLogOptions["now"];
  origin?: JourneyLogOptions["origin"];
  latencyMs?: JourneyLogOptions["latencyMs"];
  /**
   * A fault injector, for traffic drivers: asked as a node completes, and when it says yes the
   * node's record carries outcome "false" and the journey ends there FAILED (401), whatever the
   * node decided and however the tree wires "false". Not an AM behaviour; absent, nothing changes.
   */
  failAt?: (node: { tree: string; nodeId: string }) => boolean;
  /**
   * The environment's ESVs by full name (`esv.some.name`). `systemEnv.getProperty` reads them, and
   * so do `&{esv.…}` placeholders in the node configuration the emulator reads. A name that is
   * not here reads as `null` in a script, as on the tenant.
   */
  esv?: Readonly<Record<string, string | null>>;
};
const OUTCOMES: Readonly<Record<string, readonly string[]>> = Object.freeze({
  UsernameCollectorNode: ["outcome"],
  PasswordCollectorNode: ["outcome"],
  AttributeCollectorNode: ["outcome"],
  ValidatedUsernameNode: ["outcome"],
  ValidatedPasswordNode: ["outcome"],
  IdentifyExistingUserNode: ["true", "false"],
  CreateObjectNode: ["CREATED", "FAILURE"],
  PatchObjectNode: ["PATCHED", "FAILURE"],
  ...EXTRA_OUTCOMES,
});
const PAGE_CHILDREN: readonly string[] = [
  "ScriptedDecisionNode",
  "UsernameCollectorNode",
  "PasswordCollectorNode",
  "AttributeCollectorNode",
  "ValidatedPasswordNode",
  "AcceptTermsAndConditionsNode",
  "product-CaptchaNode",
  "ValidatedUsernameNode",
  "MessageNode",
  "GetAuthenticatorAppNode",
];
/**
 * A node's stored outcome vocabulary, as doc 25's listOutcomes table reports it;
 * undefined for a type the table does not cover.
 */
export function listOutcomes(
  config: JourneyConfig,
  type: string,
  node: NodeDocument,
  missing: (name: string) => never,
): readonly string[] | undefined {
  if (type === "ScriptedDecisionNode") {
    if (node.outcomes === undefined) return [];
    if (!Array.isArray(node.outcomes) || node.outcomes.some((name) => typeof name !== "string"))
      missing("ScriptedDecisionNode outcomes configuration");
    return node.outcomes;
  }
  if (type === "InnerTreeEvaluatorNode")
    return node.displayErrorOutcome ? ["true", "false", "error"] : ["true", "false"];
  if (type === "PageNode") {
    if (!Array.isArray(node.nodes) || node.nodes.length === 0) return [];
    const last = node.nodes.at(-1);
    if (!isDocument(last) || typeof last._id !== "string" || typeof last.nodeType !== "string")
      missing("PageNode declared outcome child reference");
    if (last.nodeType === "PageNode") missing("nested PageNode outcome declaration");
    const child = config.node(last.nodeType, last._id);
    if (!child) missing(`PageNode declared outcome child configuration: ${last._id}`);
    return listOutcomes(config, last.nodeType, child, missing);
  }
  if (type.startsWith("designer-")) {
    const def = config.customNodeTypes[type];
    if (!def) missing(`custom node type: ${type}`);
    return customOutcomes(def);
  }
  const dynamic = extraOutcomes(type, node);
  if (dynamic) return dynamic;
  return Object.hasOwn(OUTCOMES, type) ? OUTCOMES[type] : undefined;
}
/** The config's LIBRARY scripts keyed by script name, decoded; a script with no name cannot be required. */
function libraryBodies(config: JourneyConfig): Record<string, string> {
  const out: Record<string, string> = Object.create(null) as Record<string, string>;
  for (const script of Object.values(config.scripts)) {
    if (script.context !== "LIBRARY" || typeof script.name !== "string") continue;
    if (typeof script.script !== "string") continue;
    out[script.name] = Buffer.from(script.script, "base64").toString("utf8");
  }
  return out;
}
const PLACEHOLDER = /&\{esv\.([^}|]+)(?:\|[^}]*)?\}/g;
/**
 * A node's configuration with its ESV placeholders resolved from the configured ESVs. The tenant
 * wraps some fields as `{"$string": "&{esv.x}"}` / `{"$int": ...}` / `{"$bool": ...}` (docs/api/09-journeys.md): the
 * wrapper is read through, and a literal in one stays itself. A placeholder with no configured
 * value is missing behaviour: guessing one would make a field read as text it never held.
 */
function resolveEsv(
  config: NodeDocument,
  esv: Readonly<Record<string, string | null>>,
  missing: (name: string) => never,
): NodeDocument {
  const text = (value: string): string =>
    value.replace(PLACEHOLDER, (_all, name: string) => {
      const found = Object.hasOwn(esv, `esv.${name}`) ? esv[`esv.${name}`] : undefined;
      if (typeof found !== "string") missing(`ESV placeholder not configured: esv.${name}`);
      return found;
    });
  const walk = (value: unknown): unknown => {
    if (typeof value === "string") return text(value);
    if (Array.isArray(value)) return value.map(walk);
    if (value === null || typeof value !== "object") return value;
    const keys = Object.keys(value);
    if (keys.length === 1 && keys[0] === "$string" && typeof (value as Document).$string === "string")
      return text((value as { $string: string }).$string);
    if (keys.length === 1 && keys[0] === "$int" && typeof (value as Document).$int === "string") {
      const written = text((value as { $int: string }).$int).trim();
      // The tenant refuses a non-integer at write time (500, measured), so none reaches a run.
      if (!/^-?\d+$/.test(written)) missing("ESV placeholder resolved to a non-integer");
      return Number(written);
    }
    // Measured on a node-designer BOOLEAN property: Java's parseBoolean, so any text but "true" in any case is false.
    if (keys.length === 1 && keys[0] === "$bool" && typeof (value as Document).$bool === "string")
      return text((value as { $bool: string }).$bool).trim().toLowerCase() === "true";
    return Object.fromEntries(keys.map((key) => [key, walk((value as Document)[key])]));
  };
  return walk(config) as NodeDocument;
}
/** Session properties AM owns: measured for the first four, listed by SetSessionPropertiesNode's validation for the rest. */
const SESSION_OWNED_MEASURED = ["UserId", "AuthLevel", "Service", "Locale"];
const SESSION_OWNED = [
  ...SESSION_OWNED_MEASURED,
  ...["clientType", "FullLoginURL", "Principals", "XUSRef", "amlbcookie", "loginURL", "logoutHooks"],
  ...["HostName", "UserToken", "IndexType", "webhooks", "UserProfile", "CharSet", "authInstant"],
  ...["Host", "AMCtxId", "OidcSid", "sun.am.UniversalIdentifier", "Organization", "Principal"],
];
function mergeSession(before: SessionDelta | undefined, next: SessionDelta): SessionDelta {
  return {
    ...before,
    ...next,
    ...(before?.properties || next.properties
      ? { properties: { ...before?.properties, ...next.properties } }
      : {}),
  };
}
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
  /** LIBRARY scripts by name: `require(name)` resolves a library by its script name. */
  private readonly libraries: Record<string, string>;
  private completed = new Set<string>();
  private logs: JourneyLogs | undefined;
  constructor(
    config: JourneyConfig,
    runner: RhinoRunner,
    idm: IdmEmulator,
    options: JourneyOptions = {},
  ) {
    this.config = config;
    this.runner = runner;
    this.idm = idm;
    this.libraries = libraryBodies(config);
    const nowNs = options.now;
    this.nodes = new JourneyNodes(idm, this.evidence, (name) => this.missing(name), {
      now: nowNs ? () => Number(nowNs() / 1_000_000n) : Date.now,
      declared: (nodeId) => options.declaredOutcomes?.[nodeId],
      ...(options.termsAndConditions ? { terms: options.termsAndConditions } : {}),
    }, options.passwordPolicy, options.attributePolicies);
    for (const tree of Object.values(config.trees))
      for (const [id, node] of Object.entries(tree.nodes)) {
        if (
          Object.hasOwn(options.declaredOutcomes ?? {}, id) &&
          EMULATED.includes(node.nodeType) &&
          !STUB_TYPES.includes(node.nodeType)
        )
          throw new TypeError(`declared outcome for emulated node: ${node.nodeType}/${id}`);
      }
    for (const [type, configs] of Object.entries(config.nodes))
      if (EMULATED.includes(type) && !STUB_TYPES.includes(type))
        for (const id of Object.keys(configs)) {
          if (Object.hasOwn(options.declaredOutcomes ?? {}, id))
            throw new TypeError(`declared outcome for emulated node: ${type}/${id}`);
        }
    if (options.logSink)
      this.logs = new JourneyLogs({
        sink: options.logSink,
        ...(options.now ? { now: options.now } : {}),
        ...(options.origin ? { origin: options.origin } : {}),
        ...(options.latencyMs ? { latencyMs: options.latencyMs } : {}),
      });
    this.options = {
      ...options,
      declaredOutcomes: Object.freeze(structuredClone(options.declaredOutcomes ?? {})),
      sessions: structuredClone(options.sessions ?? {}),
    };
    if (!Number.isSafeInteger(options.timeoutMs ?? 10000) || (options.timeoutMs ?? 10000) < 0)
      throw new TypeError("timeoutMs must be a nonnegative integer");
    if (
      !Number.isSafeInteger(options.maxTransitions ?? 1000) ||
      (options.maxTransitions ?? 1000) < 1
    )
      throw new TypeError("maxTransitions must be a positive integer");
  }
  fork(): JourneyEmulator {
    const fork = new JourneyEmulator(this.config, this.runner, this.idm.fork(), this.options);
    fork.key = Buffer.from(this.key);
    fork.snapshots = structuredClone(this.snapshots);
    fork.completed = new Set(this.completed);
    fork.failures = this.failures;
    fork.logs = this.logs;
    return fork;
  }
  get emulationFailures() {
    return this.failures.entries;
  }
  private missing(name: string): never {
    this.evidence.add("missing", name);
    throw new EmulationFailure({
      kind: "missing",
      message: `Missing journey behavior: ${name}`,
      missing: [name],
    });
  }
  private measured(name: string): void {
    this.evidence.measured(name, SOURCE);
  }
  request(request: AuthenticateRequest): Promise<AuthenticateResult> {
    return this.failures.run(`authenticate ${request.query.authIndexValue}`, async () => {
      const log = this.logs?.begin(request, this.config.realm);
      log?.attempt();
      const { value, provenance } = await this.evidence.run(async () => {
        try {
          return await this.authenticate(request, log);
        } catch (error) {
          const fatal = emulationFailure(error) ?? {
            kind: "internal" as const,
            message: String(error),
          };
          for (const name of fatal.missing ?? []) this.evidence.add("missing", name);
          this.failures.record(fatal, `authenticate ${request.query.authIndexValue}`);
          return {
            status: fatal.kind === "missing" ? 501 : 500,
            body: {
              code: fatal.kind === "missing" ? 501 : 500,
              reason: "Emulation Failure",
              message: fatal.message,
            },
            emulation: fatal,
          };
        }
      });
      log?.outcome(value.status, request.query.authIndexValue);
      return { ...value, provenance };
    });
  }
  private error(status: number, message: string): Omit<AuthenticateResult, "provenance"> {
    return {
      status,
      body: {
        code: status,
        reason: status === 400 ? "Bad Request" : "Unauthorized",
        message,
      },
    };
  }
  private empty(): Omit<AuthenticateResult, "provenance"> {
    return { status: 500, body: null, rawBody: "" };
  }
  private snapshot(state: State): string {
    const id = randomUUID();
    if (state.errorMessage !== null)
      this.evidence.assumed("action error message persists across node and callback boundaries");
    this.evidence.assumed(
      "authId uses a random snapshot id and HMAC-SHA256; forks copy snapshots and share signing credentials",
    );
    const stored = structuredClone({
      ...state,
      provenance: this.evidence.entries(),
    });
    for (const frame of stored.frames) {
      frame.transient = {};
      delete frame.javaMap;
    }
    // Doc 25 probes: after a callback round trip, shared objectAttributes prints
    // in Java-map form whoever wrote it, a script included.
    stored.javaMap = isDocument(stored.shared.objectAttributes);
    this.snapshots.set(id, stored);
    return `${id}.${createHmac("sha256", this.key).update(id).digest("base64url")}`;
  }
  private restore(token: string): State | undefined {
    const [id, mac, extra] = token.split(".");
    if (!id || !mac || extra !== undefined || !/^[A-Za-z0-9_-]{43}$/.test(mac)) return undefined;
    const expected = createHmac("sha256", this.key).update(id).digest("base64url");
    if (mac.length !== expected.length || !timingSafeEqual(Buffer.from(mac), Buffer.from(expected)))
      return undefined;
    const snapshot = this.snapshots.get(id);
    return snapshot ? structuredClone(snapshot) : undefined;
  }
  private tree(name: string): TreeDocument {
    const tree = this.config.tree(name);
    if (!tree) this.missing(`inner tree configuration: ${name}`);
    for (const flag of ["noSession", "transactionalOnly", "mustRun"])
      if (tree[flag]) this.missing(`tree ${flag}`);
    for (const flag of ["maximumIdleTime", "maximumSessionTime", "treeTimeout", "staticNodes"])
      if (Object.hasOwn(tree, flag)) this.missing(`tree ${flag}`);
    if (tree.enabled === false) this.missing("disabled tree behavior");
    return tree;
  }
  private async authenticate(
    request: AuthenticateRequest,
    log?: RequestLog,
  ): Promise<Omit<AuthenticateResult, "provenance">> {
    if (
      request.method !== "POST" ||
      ![
        "authenticate",
        "/authenticate",
        `/am/json/realms/root/realms/${this.config.realm}/authenticate`,
      ].includes(request.path)
    )
      this.missing(`journey REST route: ${request.method} ${request.path}`);
    if (request.query.authIndexType !== "service")
      this.missing(`authIndexType: ${request.query.authIndexType}`);
    let body: JsonValue = request.body ?? {};
    if (request.rawBody !== undefined) {
      try {
        body = JSON.parse(request.rawBody) as JsonValue;
      } catch {
        this.missing("authenticate invalid JSON body");
      }
    }
    if (!isDocument(body)) this.missing("authenticate request body shape");
    const name = request.query.authIndexValue;
    const tree = this.config.tree(name);
    this.measured("unknown and innerTreeOnly direct journeys return No Configuration found");
    if (!tree || tree.innerTreeOnly) return this.error(400, "No Configuration found");
    let state: State;
    if (Object.hasOwn(body, "authId")) {
      if (typeof body.authId !== "string")
        return this.error(400, "Authentication Error: Invalid AuthId Signature");
      const restored = this.restore(body.authId);
      this.measured("corrupted authId returns Invalid AuthId Signature");
      if (!restored || restored.frames[0]?.tree !== name)
        return this.error(400, "Authentication Error: Invalid AuthId Signature");
      if (this.completed.has(body.authId)) this.missing("authId replay after session success");
      state = restored;
      for (const entry of restored.provenance ?? [])
        this.evidence.add(entry.kind, entry.behavior, entry.source);
      delete state.provenance;
      this.measured(
        "stale callback authId replay is accepted; shared state survives and transient is dropped",
      );
      this.evidence.assumed(
        "authId uses a random snapshot id and HMAC-SHA256; forks copy snapshots and share signing credentials",
      );
    } else {
      state = {
        shared: { realm: `/${this.config.realm}`, authLevel: 0 },
        frames: [
          {
            tree: name,
            node: tree.entryNodeId,
            transient: { maxAuthenticationSessionDuration: 300000 },
          },
        ],
        errorMessage: null,
      };
      this.evidence.assumed(
        "initial ambient state assumes shared realm/authLevel 0 and transient maxAuthenticationSessionDuration 300000; duration value and bucket are unmeasured",
      );
      const cookieName = this.options.cookieName;
      // Cookies reach nodes and scripts whatever their names. Without `cookieName` the emulator cannot say
      // which one is the session's, so it refuses only a request that carries a known session's value.
      const sessionValues = Object.keys(this.options.sessions ?? {});
      if (
        !cookieName &&
        Object.values(request.cookies ?? {}).some((value) => sessionValues.includes(value))
      )
        this.missing("session cookie name not supplied");
      const cookie = cookieName ? request.cookies?.[cookieName] : undefined;
      if (cookie) {
        const sessions = this.options.sessions;
        if (!sessions || !Object.hasOwn(sessions, cookie))
          this.missing("existingSession cookie not supplied in session fixtures");
        state.existingSession = structuredClone(sessions[cookie]) as Record<string, string>;
        this.evidence.assumed(
          "existingSession is the caller-supplied complete cookie session snapshot",
        );
      }
      if (body.callbacks !== undefined) {
        // Doc 25 measured this with a scripted entry node only.
        const entry = tree.nodes[tree.entryNodeId]?.nodeType;
        if (
          Array.isArray(body.callbacks) &&
          body.callbacks.length &&
          entry !== "ScriptedDecisionNode"
        )
          this.missing(`callbacks without authId to a ${String(entry)} entry node`);
        this.measured("callbacks without authId start a fresh journey and reach its first node");
      }
    }
    if (log && this.logs) {
      state.tracking ??= this.logs.nextId();
      log.tracking = state.tracking;
    }
    const rootTree = state.frames[0]?.tree ?? name;
    let submitted = fromWire(body.callbacks, (name) => this.missing(name));
    if (state.pending) {
      const raw = body.callbacks;
      if (!Array.isArray(raw) || raw.length !== state.pending.length)
        this.missing("authenticate callback submission differs from snapshot");
      for (const [index, expected] of state.pending.entries()) {
        const actual = raw[index];
        if (
          !isDocument(actual) ||
          actual.type !== expected.type ||
          JSON.stringify(actual.output) !== JSON.stringify(expected.output)
        )
          this.missing("authenticate callback submission differs from snapshot");
        const names = Array.isArray(actual.input)
          ? actual.input.map((item) => (isDocument(item) ? item.name : null))
          : [];
        if (
          JSON.stringify(names) !== JSON.stringify((expected.input ?? []).map((item) => item.name))
        )
          this.missing("authenticate callback submission differs from snapshot");
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
          state.frames.pop();
          const parent = state.frames.at(-1);
          if (!parent) throw new Error("inner frame invariant");
          this.measured(
            "inner success/failure maps to true/false and restores outer transient state",
          );
          // UNMEASURED: whether the inner-tree node writes its own completion record, and under
          // which tree name. It is written here, as the outer tree's, once the inner tree ends.
          const evaluator = this.tree(parent.tree).nodes[parent.node];
          if (evaluator)
            log?.nodeCompleted({
              tree: parent.tree,
              nodeId: parent.node,
              displayName: evaluator.displayName ?? evaluator.nodeType,
              nodeType: evaluator.nodeType,
              outcome: success ? "true" : "false",
              authLevel: state.shared.authLevel,
            });
          const reply = this.advance(state, success ? "true" : "false");
          if (reply) return reply;
          continue;
        }
        this.measured(
          "failure is 401 with action error message; success requires shared username or returns empty 500",
        );
        if (!success) {
          log?.treeFailed(rootTree, state.shared.authLevel);
          return this.error(401, state.errorMessage ?? "Login failure");
        }
        // UNMEASURED: an empty 500 writes no tree record here; the tenant's is unread.
        if (!state.shared.username) return this.empty();
        this.evidence.assumed(
          "session tokenId is a local random UUID; principal existence and session properties are not modeled",
        );
        if (this.config.realm !== "alpha")
          this.missing(`session success realm: ${this.config.realm}`);
        if (typeof body.authId === "string") this.completed.add(body.authId);
        log?.treeSucceeded(
          rootTree,
          state.shared.authLevel,
          String(state.shared.username),
          typeof state.shared._id === "string" ? state.shared._id : undefined,
        );
        const tokenId = randomUUID();
        const session = state.session;
        return {
          status: 200,
          body: {
            tokenId,
            successUrl: Object.hasOwn(state.shared, "successUrl")
              ? (state.shared.successUrl as JsonValue)
              : "/enduser/?realm=/alpha",
            realm: "/alpha",
          },
          ...(session
            ? {
                session: {
                  properties: session.properties ?? {},
                  ...(session.maxIdleTime !== undefined
                    ? { maxIdleTime: session.maxIdleTime }
                    : {}),
                  ...(session.maxSessionTime !== undefined
                    ? { maxSessionTime: session.maxSessionTime }
                    : {}),
                },
                ...(session.cookie ? { cookies: [session.cookie] } : {}),
              }
            : {}),
        };
      }
      const node = currentTree.nodes[frame.node];
      if (!node) this.missing(`tree node reference: ${frame.tree}/${frame.node}`);
      const config = this.config.node(node.nodeType, frame.node);
      if (!config) this.missing(`node configuration: ${node.nodeType}/${frame.node}`);
      if (node.nodeType === "InnerTreeEvaluatorNode") {
        if (typeof config.tree !== "string") this.missing("inner tree target configuration");
        const innerTree = this.tree(config.tree);
        this.measured(
          "inner trees share shared state and use isolated transient state; journey names follow the executing tree; callbacks inside an inner tree resume there, and inner trees nest",
        );
        state.frames.push({
          tree: config.tree,
          node: innerTree.entryNodeId,
          transient: {},
        });
        submitted = [];
        continue;
      }
      const result = await this.execute(
        node.nodeType,
        frame.node,
        config,
        state,
        frame,
        submitted,
        request,
        log,
      );
      if (result.abort) return result.abort;
      if (result.callbacks.length) {
        this.measured(
          "queued callbacks supersede goTo; node re-executes on resume with shared state preserved",
        );
        this.evidence.assumed(
          "basic callback wire envelopes reuse binding effect fields; input defaults use empty strings except HiddenValue id and TextInput defaultText",
        );
        const callbacks = toWire(result.callbacks, (name) => this.missing(name));
        state.pending = callbacks;
        return {
          status: 200,
          body: {
            authId: this.snapshot(state),
            callbacks,
            ...(result.header !== undefined ? { header: result.header } : {}),
            ...(result.description !== undefined ? { description: result.description } : {}),
            ...(result.stage !== undefined ? { stage: result.stage } : {}),
          },
        };
      }
      const outcome = result.outcome;
      const allowed = this.outcomes(node.nodeType, config);
      if (outcome === null || (allowed !== undefined && !allowed.includes(outcome))) {
        this.measured("undeclared node outcome returns 401 Login failure");
        // UNMEASURED: no node record, and a plain "Login failure" tree record, for an outcome the
        // node does not declare.
        log?.treeFailed(rootTree, state.shared.authLevel);
        return this.error(401, "Login failure");
      }
      if (this.options.failAt?.({ tree: frame.tree, nodeId: frame.node })) {
        log?.nodeCompleted({
          tree: frame.tree,
          nodeId: frame.node,
          displayName: node.displayName ?? node.nodeType,
          nodeType: node.nodeType,
          outcome: "false",
          authLevel: state.shared.authLevel,
        });
        log?.treeFailed(rootTree, state.shared.authLevel);
        return this.error(401, "Login failure");
      }
      // A node that queued callbacks has not completed and wrote nothing; it re-runs on resume.
      log?.nodeCompleted({
        tree: frame.tree,
        nodeId: frame.node,
        displayName: node.displayName ?? node.nodeType,
        nodeType: node.nodeType,
        outcome,
        authLevel: state.shared.authLevel,
        ...(result.audit === undefined ? {} : { audit: result.audit }),
      });
      const reply = this.advance(state, outcome);
      if (reply) return reply;
      submitted = [];
    }
    throw new EmulationFailure({
      kind: "protocol",
      message: "journey transition limit exceeded",
    });
  }
  private async scripted(
    config: NodeDocument,
    state: State,
    frame: Frame,
    submitted: CallbackEffect[],
    request: AuthenticateRequest,
    log?: RequestLog,
    synthetic?: ScriptDocument,
  ): Promise<NodeResult> {
    const currentTree = this.tree(frame.tree),
      inner = state.frames.length > 1;
    for (const key of synthetic ? [] : ["inputs", "outputs"]) {
      const selectors = config[key];
      if (
        selectors !== undefined &&
        (!Array.isArray(selectors) || selectors.length !== 1 || selectors[0] !== "*")
      )
        this.missing(`ScriptedDecisionNode ${key} filtering`);
    }
    const script =
      synthetic ??
      (typeof config.script === "string" ? this.config.script(config.script) : undefined);
    if (!script) this.missing(`script configuration: ${String(config.script)}`);
    if (script.evaluatorVersion !== "2.0")
      this.missing(`script evaluatorVersion: ${script.evaluatorVersion}`);
    if (script.language && script.language !== "JAVASCRIPT")
      this.missing(`script language: ${script.language}`);
    if (
      typeof script.script !== "string" ||
      Buffer.from(script.script, "base64").toString("base64") !== script.script
    )
      this.missing("script base64 encoding");
    this.evidence.assumed(
      "AM binding facsimiles reuse rhino-local's measured overlay; Java collection wrappers remain local approximations",
    );
    this.evidence.assumed(
      "nodeState transient-first whole-value shadowing generalizes measured objectAttributes behavior to other keys",
    );
    this.measured(
      "next-gen nodeState has no getShared/getTransient; callback-free boundaries retain transient state",
    );
    if (
      (state.javaMap && isDocument(state.shared.objectAttributes)) ||
      (frame.javaMap && isDocument(frame.transient.objectAttributes))
    )
      this.evidence.assumed(
        "objectAttributes written by a node or carried across a round trip is a Java map in later scripts; its printed form is measured, its get/put are inferred",
      );
    this.measured(
      "action withErrorMessage works in either call order; withLockoutMessage has no visible failure-message effect",
    );
    const response = await this.runner.eval(
      {
        engine: "am",
        hostCalls: true,
        timeoutMs: this.options.timeoutMs ?? 10000,
        source: Buffer.from(script.script, "base64").toString("utf8"),
        sourceName: script.name ?? config.script ?? "script",
        classAllowList: decisionNodeClassAllowList(),
        ...journeyPreamble({
          shared: state.shared,
          transient: frame.transient,
          callbacks: submitted,
          tree: frame.tree,
          inner,
          identityResource: currentTree.identityResource ?? `managed/${this.config.realm}_user`,
          realm: this.config.realm,
          scriptName: script.name ?? config.script ?? "script",
          scriptId: config.script ?? "script",
          ...(state.existingSession ? { existingSession: state.existingSession } : {}),
          ...(state.session?.properties ? { priorSessionProperties: state.session.properties } : {}),
          javaMaps: {
            shared: Boolean(state.javaMap),
            transient: Boolean(frame.javaMap),
          },
          libraries: this.libraries,
          esv: { ...this.options.esv },
          ...(request.headers ? { headers: { ...request.headers } } : {}),
          ...(request.cookies ? { cookies: { ...request.cookies } } : {}),
          parameters: Object.fromEntries(
            Object.entries(request.query).map(([key, value]) => [
              key,
              typeof value === "string" ? [value] : [...value],
            ]),
          ),
        }),
      },
      { onCall: this.evidence.bind((call) => this.host(call)) },
    );
    for (const entry of response.provenance ?? [])
      this.evidence.add(entry.kind, entry.behavior, entry.source);
    const fatal = responseFailure(response);
    if (fatal) throw new EmulationFailure(fatal);
    if (response.outcome === "compile_error") this.missing("journey script compilation failure");
    const scriptName = script.name ?? config.script ?? "script";
    log?.scriptLines(response.logs ?? [], String(config.script ?? "script"), scriptName);
    if (response.outcome !== "ok") {
      if (inner) this.missing("inner tree script exception");
      this.measured("thrown scripted decision error returns 401 Login failure");
      if (log)
        log.treeFailed(
          state.frames[0]?.tree ?? frame.tree,
          state.shared.authLevel,
          log.scriptThrew({
            scriptName,
            thrown: thrownText(response),
            line: response.error?.line ?? 1,
            column: Math.max(response.error?.column ?? 0, 0),
          }),
        );
      return { outcome: null, callbacks: [], abort: this.error(401, "Login failure") };
    }
    // The runner's bridge serializes the harvest in Java and fails it as an emulation failure.
    if (typeof response.value !== "string") this.missing("journey harvest serialization");
    const result = JSON.parse(response.value) as {
      effects: {
        sharedState: { final: Document };
        transientState: { final: Document };
        sessionProperties: Record<string, string>;
        callbacks: CallbackEffect[];
        outcome: string | null;
        javaMapObjectAttributes: { shared: boolean; transient: boolean };
      };
      errorMessage: string | null;
      auditEntryDetail?: { value?: JsonValue; invalid?: true };
    };
    // AM fails the tree on an `auditEntryDetail` that is not a string or a map, and writes no node
    // record for it (measured 2026-11-25, docs/api/25-journey-runtime.md).
    if (result.auditEntryDetail?.invalid) {
      this.measured("an auditEntryDetail that is not a string or a map fails the tree with 401");
      if (inner) this.missing("inner tree script exception");
      if (log)
        log.treeFailed(
          state.frames[0]?.tree ?? frame.tree,
          state.shared.authLevel,
          log.invalidAuditDetail(),
        );
      return { outcome: null, callbacks: [], abort: this.error(401, "Login failure") };
    }
    // AM discards the Action when callbacks are queued; whether a message set
    // alongside them survives is unmeasured.
    if (result.errorMessage !== null && result.effects.callbacks.length)
      this.missing("action.withErrorMessage with queued callbacks");
    const sessionAbort = this.scriptSessionProperties(result.effects.sessionProperties, state);
    if (sessionAbort) {
      if (inner) this.missing("inner tree script exception");
      return { outcome: null, callbacks: [], abort: sessionAbort };
    }
    state.shared = result.effects.sharedState.final;
    frame.transient = result.effects.transientState.final;
    state.javaMap = result.effects.javaMapObjectAttributes.shared;
    frame.javaMap = result.effects.javaMapObjectAttributes.transient;
    if (result.errorMessage !== null) state.errorMessage = result.errorMessage;
    return {
      outcome: result.effects.outcome,
      callbacks: result.effects.callbacks,
      ...(result.auditEntryDetail?.value === undefined
        ? {}
        : { audit: result.auditEntryDetail.value }),
    };
  }
  /**
   * What a script's `putSessionProperty` / `removeSessionProperty` leave on the session the
   * journey makes (docs/api/25-journey-runtime.md, Session properties from scripts, 2026-11-25).
   * Returns the abort when the script put a property AM owns.
   */
  private scriptSessionProperties(
    final: Record<string, string>,
    state: State,
  ): Omit<AuthenticateResult, "provenance"> | undefined {
    const incoming = state.existingSession ?? {};
    const prior = state.session?.properties ?? {};
    const next: Record<string, string> = {};
    for (const [key, value] of Object.entries(final)) {
      if (incoming[key] === value && prior[key] === undefined) continue;
      if (prior[key] === value) {
        next[key] = value;
        continue;
      }
      if (SESSION_OWNED_MEASURED.includes(key)) {
        this.measured(
          "putSessionProperty of a property AM owns fails the journey with 401 Login failure",
        );
        return this.error(401, "Login failure");
      }
      if (SESSION_OWNED.includes(key))
        this.missing(`putSessionProperty of a property AM owns: ${key}`);
      next[key] = value;
    }
    for (const key of Object.keys(incoming))
      if (!Object.hasOwn(final, key) && prior[key] === undefined)
        this.missing("removeSessionProperty of a property on the incoming session");
    if (Object.keys(next).length || Object.keys(prior).length) {
      this.measured(
        "a script's putSessionProperty reaches the session the journey makes (values as strings, later writes win, removeSessionProperty drops an earlier node's property, kept across a callback round trip)",
      );
      state.session = { ...state.session, properties: next };
    }
    return undefined;
  }
  private async execute(
    type: string,
    id: string,
    config: NodeDocument,
    state: State,
    frame: Frame,
    submitted: CallbackEffect[],
    request: AuthenticateRequest,
    log?: RequestLog,
  ): Promise<NodeResult> {
    if (type === "ScriptedDecisionNode")
      return this.scripted(config, state, frame, submitted, request, log);
    if (type === "PageNode") return this.page(config, state, frame, submitted, request, log);
    if (type.startsWith("designer-")) {
      const def = this.config.customNodeTypes[type];
      if (!def) this.missing(`custom node type: ${type}`);
      const resolved = resolveEsv(config, this.options.esv ?? {}, (name) => this.missing(name));
      const built = customNodeSource(def, resolved, (name) => this.missing(name));
      this.evidence.measured(
        "a custom node runs its type's script with the instance's properties bound; a throw, or an outcome the type does not list, takes Script Error when the type has an error outcome, else fails the journey with 401 Login failure; a missing required property fails it with 401",
        "docs/api/25-journey-runtime.md (custom node types, 2026-11-25)",
      );
      this.evidence.assumed(
        "a custom node's outputs list does not restrict its writes and an undeclared outcome without an error outcome fails like a throw; unmeasured",
      );
      if ("abort" in built)
        return { outcome: null, callbacks: [], abort: this.error(401, "Login failure") };
      return this.scripted({ script: type }, state, frame, submitted, request, log, {
        script: Buffer.from(built.source).toString("base64"),
        evaluatorVersion: "2.0",
        name: typeof def.displayName === "string" ? def.displayName : type,
      });
    }
    if (EMULATED.includes(type)) {
      const resolved = resolveEsv(config, this.options.esv ?? {}, (name) => this.missing(name));
      const before = [state.shared.objectAttributes, frame.transient.objectAttributes];
      const result = await this.nodes.run(
        type,
        resolved,
        state.shared,
        frame.transient,
        submitted,
        this.tree(frame.tree).identityResource ?? `managed/${this.config.realm}_user`,
        id,
        request.cookies,
      );
      if (result.session) state.session = mergeSession(state.session, result.session);
      // Doc 25 probes: objectAttributes a node writes prints in Java-map form in the next script.
      if (JSON.stringify(state.shared.objectAttributes) !== JSON.stringify(before[0]))
        state.javaMap = isDocument(state.shared.objectAttributes);
      if (JSON.stringify(frame.transient.objectAttributes) !== JSON.stringify(before[1]))
        frame.javaMap = isDocument(frame.transient.objectAttributes);
      return result;
    }
    const declarations = this.options.declaredOutcomes;
    if (!declarations || !Object.hasOwn(declarations, id)) this.missing(`node type: ${type}`);
    const outcome = declarations[id] ?? null;
    this.evidence.assumed(`declared by test: ${frame.tree}/${id} -> ${String(outcome)}`);
    return { outcome, callbacks: [] };
  }
  /**
   * A Page's localized `pageHeader` / `pageDescription` map as the wire string (doc 25, 2026-11-25): the
   * `en` entry, else the only entry, an empty string meaning none. Which of several locales an
   * Accept-Language request picks is unmeasured.
   */
  private pageText(
    config: NodeDocument,
    field: "pageHeader" | "pageDescription",
    wire: "header" | "description",
    request: AuthenticateRequest,
  ): { header?: string; description?: string } {
    const map = config[field];
    if (map === undefined) return {};
    if (!isDocument(map)) this.missing(`PageNode ${field} shape`);
    const keys = Object.keys(map);
    if (!keys.length) return {};
    const lang = Object.keys(request.headers ?? {}).some(
      (h) => h.toLowerCase() === "accept-language",
    );
    if (lang && keys.length > 1)
      this.missing(`PageNode ${field} with several locales and Accept-Language`);
    const key = Object.hasOwn(map, "en") ? "en" : keys.length === 1 ? keys[0] : undefined;
    if (key === undefined) this.missing(`PageNode ${field} with several locales and no en`);
    const value = map[key];
    if (typeof value !== "string") this.missing(`PageNode ${field} value`);
    this.measured(
      "a PageNode sends pageHeader and pageDescription as header and description beside its callbacks: the en entry, else a sole entry, omitted when empty; a re-sent page repeats them",
    );
    return value === "" ? {} : { [wire]: value };
  }
  /** A Page's `stage` string as the top-level `stage`; empty or absent sends none (doc 25, 2026-11-25). */
  private pageStage(config: NodeDocument): { stage?: string } {
    const stage = config.stage;
    if (stage === undefined || stage === "") return {};
    if (typeof stage !== "string") this.missing("PageNode stage shape");
    this.measured("a PageNode sends its non-empty stage as a top-level stage beside callbacks; a re-sent page repeats it");
    return { stage };
  }
  private async page(
    config: NodeDocument,
    state: State,
    frame: Frame,
    submitted: CallbackEffect[],
    request: AuthenticateRequest,
    log?: RequestLog,
  ): Promise<NodeResult> {
    // A Page in an inner tree is the same machinery: this tree's own properties, no outer page's
    // (doc 25, "A Page inside an inner tree", 2026-11-25).
    const texts = {
      ...this.pageText(config, "pageHeader", "header", request),
      ...this.pageText(config, "pageDescription", "description", request),
      ...this.pageStage(config),
    };
    if (!Array.isArray(config.nodes) || !config.nodes.length)
      this.missing("PageNode child configuration");
    const previous = state.page;
    delete state.page;
    if (!previous && submitted.length) this.missing("PageNode callbacks without authId");
    const counts: number[] = [],
      collected: CallbackEffect[] = [];
    let offset = 0,
      outcome: string | null = null,
      scriptRan = false,
      rejected = false;
    for (const [index, child] of config.nodes.entries()) {
      if (!isDocument(child) || typeof child._id !== "string" || typeof child.nodeType !== "string")
        this.missing("PageNode child reference");
      if (child.nodeType === "PageNode") this.missing("nested PageNode");
      // Doc 25 measured collectors and scripted decisions only; any other child is unmeasured.
      if (!PAGE_CHILDREN.includes(child.nodeType))
        this.missing(`PageNode child type: ${child.nodeType}`);
      const childConfig = this.config.node(child.nodeType, child._id);
      if (!childConfig) this.missing(`PageNode child configuration: ${child._id}`);
      // Measured with validateInput false only: the policy callback of `true` is not modelled anywhere.
      if (child.nodeType === "ValidatedUsernameNode" && childConfig.validateInput !== false)
        this.missing("ValidatedUsernameNode validateInput: true as a PageNode child");
      const count = previous?.counts[index] ?? 0;
      const own = previous ? submitted.slice(offset, offset + count) : [];
      offset += count;
      // Doc 25 (discovery rows): every scripted child runs for real when the page
      // is first reached, callback-free or not, and again on submission.
      if (previous && child.nodeType === "ScriptedDecisionNode") {
        // A sibling already rejected this submission: whether AM still runs a
        // later script, and so commits its effects, is unmeasured.
        if (rejected)
          this.missing("PageNode scripted child after a rejected sibling on submission");
        scriptRan = true;
      }
      const message = state.errorMessage;
      const result = await this.execute(
        child.nodeType,
        child._id,
        childConfig,
        state,
        frame,
        own,
        request,
        log,
      );
      if (result.abort) {
        if (!previous)
          this.measured("a throw in a Page child's discovery run fails the first POST with 401");
        return result;
      }
      if (
        previous &&
        child.nodeType === "product-CaptchaNode" &&
        own.some((cb) => cb.type === "ReCaptchaCallback" && cb.value === "")
      ) {
        this.measured(
          "a Page whose product-CaptchaNode child is submitted with an empty token fails with 401 Login failure",
        );
        return { outcome: null, callbacks: [], abort: this.error(401, "Login failure") };
      }
      // Discovery ignores the child's Action (doc 25); whether a message set there survives is unmeasured.
      if (!previous && state.errorMessage !== message)
        this.missing("action.withErrorMessage in a PageNode discovery run");
      if (previous && result.callbacks.length) rejected = true;
      counts.push(result.callbacks.length);
      collected.push(...result.callbacks);
      outcome = result.outcome;
    }
    this.measured(
      "PageNode children run in order with only their own callbacks; last child supplies the outcome",
    );
    if (!previous) {
      if (!collected.length) {
        // Doc 25 (2026-11-25): a page whose children queue no callbacks runs each child once and leaves
        // by the last child's outcome, with nothing sent to the client.
        if (outcome === null) this.missing("PageNode without callbacks and without an outcome");
        this.measured(
          "a PageNode whose children queue no callbacks runs them once and passes through by the last child's outcome",
        );
        return { outcome, callbacks: [] };
      }
      state.page = { counts, effects: structuredClone(collected) };
      // Measured: a page whose only child is a Message or Get Authenticator App node leaves no
      // pageNodeCallbacks at all (doc 25, "Nodes as Page children").
      const display = config.nodes.length === 1 && isDocument(config.nodes[0]) &&
        ["MessageNode", "GetAuthenticatorAppNode"].includes(String(config.nodes[0].nodeType));
      if (!display) {
        state.shared.pageNodeCallbacks = toWire(collected, (name) => this.missing(name)) as JsonValue;
        this.evidence.assumed(
          "pageNodeCallbacks stores the merged callback wire array; only shared presence and lifetime are measured (the tenant's value reads as a map of callback index to child index)",
        );
      }
      if (
        config.nodes.some((child) => isDocument(child) && child.nodeType === "ScriptedDecisionNode")
      )
        this.measured(
          "PageNode discovery runs every scripted child once, with real effects and retained state, and ignores its goTo; submission runs it again",
        );
      return { outcome: null, callbacks: collected, ...texts };
    }
    if (collected.length) {
      // Doc 25 measured a re-send only for collectors; what a script's effects do
      // across a re-sent page is unmeasured.
      if (scriptRan) this.missing("PageNode re-sent after a scripted child ran on submission");
      // A rejected collector re-sends the entire page. Doc 25 (out-coll page
      // step 2): a StringAttributeInputCallback echoes its submitted value; a
      // PasswordCallback goes out as first sent, prompt only, never the value.
      const retry: CallbackEffect[] = [];
      let start = 0,
        firstShape = false;
      for (const [index, count] of previous.counts.entries()) {
        const sent = previous.effects.slice(start, start + count);
        const answered = submitted.slice(start, start + count);
        if (counts[index]) {
          if (counts[index] !== count)
            this.missing("PageNode callback count changes on resubmission");
          retry.push(...collected.splice(0, count));
        } else if (
          answered.every(
            (cb) => cb.type === "StringAttributeInputCallback" || cb.type === "NameCallback",
          )
        ) {
          retry.push(...answered.map((cb) => ({ ...cb })));
        } else {
          if (sent.some((cb) => cb.type !== "PasswordCallback")) firstShape = true;
          retry.push(...structuredClone(sent));
        }
        start += count;
      }
      if (firstShape)
        this.evidence.assumed(
          "a re-sent Page repeats an accepted sibling's callbacks as first sent; measured for PasswordCallback only",
        );
      state.page = previous;
      state.shared.pageNodeCallbacks = toWire(retry, (name) => this.missing(name)) as JsonValue;
      this.evidence.assumed(
        "pageNodeCallbacks stores the merged callback wire array; only shared presence and lifetime are measured",
      );
      this.measured(
        "required empty attribute re-sends the whole page with echoed values and empty failedPolicies",
      );
      return { outcome: null, callbacks: retry, ...texts };
    }
    return { outcome, callbacks: [] };
  }
  private outcomes(type: string, config: NodeDocument): readonly string[] | undefined {
    return listOutcomes(this.config, type, config, (name) => this.missing(name));
  }
  private advance(
    state: State,
    outcome: string,
  ): Omit<AuthenticateResult, "provenance"> | undefined {
    const frame = state.frames.at(-1);
    if (!frame) throw new Error("journey advance frame invariant");
    const node = this.tree(frame.tree).nodes[frame.node];
    if (!node) throw new Error("journey advance node invariant");
    if (!Object.hasOwn(node.connections, outcome)) {
      this.measured("declared but unwired node outcome returns empty 500");
      return this.empty();
    }
    if (state.errorMessage !== null)
      this.evidence.assumed("action error message persists across node and callback boundaries");
    frame.node = node.connections[outcome] ?? "";
    return undefined;
  }
  private checkHostValue(value: JsonValue): void {
    try {
      JSON.stringify(value, (_key, item: unknown) => {
        if (typeof item === "number" && (!Number.isFinite(item) || Object.is(item, -0)))
          this.missing("AM host non-finite or negative-zero result");
        if (["undefined", "function", "symbol", "bigint"].includes(typeof item))
          this.missing("AM host non-JSON result");
        return item;
      });
    } catch (error) {
      if (emulationFailure(error)) throw error;
      throw new EmulationFailure({
        kind: "protocol",
        message: "AM host result is not JSON serializable",
      });
    }
  }

  private async host(call: HostCall): Promise<JsonValue> {
    if (call.method !== "http") {
      try {
        const result = await this.idm.onCall(call);
        this.checkHostValue(result);
        return result;
      } catch (error) {
        if (error instanceof HostCallError)
          this.evidence.assumed(
            "AM openidm errors use a JavaException-named JS adapter with string code and JSON detail",
          );
        throw error;
      }
    }
    const [url, options, wire] = call.args;
    if (
      typeof url !== "string" ||
      !isDocument(options) ||
      !isDocument(wire) ||
      !Array.isArray(wire.assumed) ||
      wire.assumed.some((reason) => typeof reason !== "string") ||
      !(wire.body === undefined || typeof wire.body === "string")
    )
      this.missing("httpClient request shape");
    if (!this.options.http) this.missing(`unmocked httpClient.send: ${url}`);
    this.evidence.assumed(
      "httpClient response is supplied by the test mock; get/json/text wrappers reuse AM binding shapes",
    );
    // Java names each way this request leaves the measured shape; only a request inside it
    // (an object body sent as application/json, strings in the classes doc 12 §4 sampled) gets
    // the measured label.
    if (wire.assumed.length === 0)
      this.evidence.measured(
        'httpClient body text: an object body sent as Content-Type: application/json renders JS numbers as Java doubles (1 is 1.0) and undefined as null, and escapes strings and keys as measured (in printable ASCII only " and \\; other BMP raw; \\n, \\t short; other controls, astral and lone high surrogates as upper-case \\u escapes); no body key is empty',
        HTTP_BODY_SOURCE,
      );
    for (const reason of wire.assumed as string[]) this.evidence.assumed(reason);
    let response: Awaited<ReturnType<HttpMock>>;
    try {
      response = await this.options.http(url, options, {
        bodyText: wire.body ?? "",
        ...(typeof wire.contentType === "string" ? { contentType: wire.contentType } : {}),
      });
    } catch (error) {
      throw new EmulationFailure(
        emulationFailure(error) ?? {
          kind: "internal",
          message: `journey HTTP mock failed: ${String(error)}`,
        },
      );
    }
    if (response && "failure" in response) {
      const { failure } = response;
      const host = new URL(url).hostname;
      const tail =
        failure.kind === "refused"
          ? "java.net.ConnectException: Connection refused (connect failed)"
          : failure.kind === "unknown-host"
            ? `java.net.UnknownHostException: ${host}: Name or service not known`
            : failure.kind === "connect-timeout" &&
                typeof failure.address === "string" &&
                Number.isInteger(failure.port)
              ? `java.net.ConnectException: Timeout connecting to [/${failure.address}:${failure.port}]`
              : undefined;
      if (tail === undefined) this.missing(`httpClient mock failure: ${url}`);
      const result = { failure: tail };
      this.checkHostValue(result);
      return result;
    }
    if (
      !response ||
      !Number.isInteger(response.status) ||
      response.status < 100 ||
      response.status > 599
    )
      this.missing(`httpClient mock response: ${url}`);
    const result = {
      status: response.status,
      body: response.body ?? null,
      headers: response.headers ?? {},
    };
    this.checkHostValue(result);
    return result;
  }
}

/**
 * Rhino's own text for a throw, `<thrown value> (<script>#<line>)`: the tenant's ScriptException
 * message carries exactly that (`Error: boom (name#1)`; a thrown string reads `str (name#1)`).
 */
function thrownText(response: { error: { message: string | null } | null }): string {
  return response.error?.message ?? "unknown error";
}
