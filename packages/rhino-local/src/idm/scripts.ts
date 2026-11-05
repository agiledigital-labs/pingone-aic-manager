/** Hook binding graphs and IDM evaluations. Host calls re-enter the owning emulator. */
import type { EvalOptions, RhinoRunner } from "../runner.ts";
import type { HostCall, JsonValue, JobResponse } from "../protocol.ts";
import { isDocument, own, replaceDocument, setOwn } from "./document.ts";
import { missing, scriptError } from "./errors.ts";
import type { Evidence } from "./provenance.ts";
import { modulePath, type RuntimeScripts } from "./runtime-scripts.ts";
import type { Document, Hook, ManagedType, Script } from "./types.ts";

export type Invocation = {
  request: Document;
  context: Document;
  resourceName: string;
  oldObject: Document | null;
  object: Document | null;
  newObject: Document | null;
};

export class Scripts {
  private readonly runner: RhinoRunner;
  private readonly evidence: Evidence;
  private readonly environmentProperties: Record<string, JsonValue>;
  private readonly runtime: RuntimeScripts;
  private readonly onCall: (call: HostCall, context: Document) => ReturnType<NonNullable<EvalOptions["onCall"]>>;
  constructor(
    runner: RhinoRunner,
    evidence: Evidence,
    onCall: (call: HostCall, context: Document) => ReturnType<NonNullable<EvalOptions["onCall"]>>,
    properties: Record<string, JsonValue>,
    runtime: RuntimeScripts,
  ) {
    this.runner = runner;
    this.runtime = runtime;
    this.evidence = evidence;
    this.onCall = onCall;
    this.environmentProperties = properties;
  }

  async evaluate(script: Script, bindings: Document, context: string, bindingAliases?: Record<string, string>): Promise<JobResponse> {
    if (script.type !== "text/javascript") missing(this.evidence, `script type: ${script.type}`);
    if (script.file !== undefined && script.source !== undefined) missing(this.evidence, `script declaring both source and file: ${script.file}`);
    // A `file` names one of IDM's own runtime scripts (AIC has no customer script directory).
    const source = script.file === undefined ? script.source :
      await this.runtime.source(script.file, `${context} file reference`, this.evidence);
    if (source === undefined) missing(this.evidence, "script without inline source");
    if (script.file !== undefined) this.evidence.assumed("a file-referenced script gets the bindings an inline script in the same place gets");
    this.evidence.measured("scripts execute on the IDM engine with resource-name binding");
    this.evidence.measured("request uses a CREST method request wrapper");
    this.evidence.assumed("Java long values use JavaScript number precision");
    const response = await this.runner.eval({
      ...(bindingAliases ? { bindingAliases } : {}),
      properties: this.environmentProperties, internalValues: true,
      engine: "idm", source, sourceName: script.file ?? context, idmRequire: true,
      bindings: { ...script.globals, ...bindings }, resourceNameBindings: ["resourceName"], reportMissing: true, crestBindings: true,
    }, { onCall: this.evidence.bind((call) => this.onCall(call, isDocument(bindings.context) ? bindings.context : {})) });
    for (const measurement of response.measured ?? []) this.evidence.measured(measurement, "docs/api/24-idm-script-runtime.md");
    for (const entry of response.provenance ?? []) if (entry.kind !== "missing") this.evidence.add(entry.kind, entry.behavior, entry.source);
    for (const assumption of response.assumed ?? []) this.evidence.assumed(assumption);
    for (const helper of response.missing ?? []) this.evidence.add("missing", `runtime helper: ${helper}`);
    const absent = this.evidence.firstMissing();
    if (absent) missing(this.evidence, absent.behavior);
    if (response.outcome !== "ok") scriptError(response, context, this.evidence);
    return response;
  }

  /**
   * Answers the engine's `require(id)` host call with the module's IDM path
   * and source from the runtime-script provider, or the fault naming it.
   */
  async require(args: readonly JsonValue[]): Promise<JsonValue> {
    const id = args[0];
    if (typeof id !== "string" || args.length !== 1) missing(this.evidence, "require host call without one string id");
    this.evidence.assumed("require(id) loads <id>.js under the script root; IDM 9.0.0 bytecode, not measured on AIC");
    this.evidence.assumed("a required module runs once per script invocation, sees the caller's bindings and keeps its top-level vars; IDM 9.0.0 bytecode, not measured on AIC");
    const path = modulePath(id);
    if (["lib/lodash.js", "lib/handlebars.js", "lib/validator.js"].includes(path)) {
      this.evidence.assumed("engine-preloaded lib/lodash, lib/handlebars and lib/validator run per invocation here; IDM shares one exports object across invocations");
    }
    return { path, source: await this.runtime.source(path, `require(${JSON.stringify(id)})`, this.evidence) };
  }

  async hook(type: ManagedType, hook: Hook, invocation: Invocation): Promise<void> {
    const script = own(type, hook);
    if (!script) return;
    const { object, request, context, resourceName, oldObject, newObject } = invocation;
    const bindings: Document = { object, request, context, resourceName };
    if (hook === "onValidate" || hook === "onStore") bindings.value = object;
    else Object.assign(bindings, { oldObject, newObject });
    if (hook === "onValidate" || hook === "onStore") this.evidence.assumed("object validation/store value binding uses the current object snapshot");
    const aliases: Record<string, string> = {};
    if ((hook === "onCreate" || hook === "onUpdate") && object !== null && newObject === object) {
      aliases.newObject = "object";
    }
    const response = await this.evaluate(script, bindings, hook, aliases);
    if (object !== null && isDocument(response.bindings?.object)) {
      replaceDocument(object, response.bindings.object);
    }
  }

  async properties(type: ManagedType, hook: "onValidate" | "onStore" | "onRetrieve", invocation: Invocation, selectors: string[] = []): Promise<void> {
    const object = invocation.object;
    if (!object) return;
    for (const [name, config] of Object.entries(type.schema.properties)) {
      const script = own(config, hook);
      if (!script) continue;
      if (hook === "onRetrieve" && selectors.length > 0 && !selectors.includes("*") &&
        !selectors.some((field) => field.replace(/^\//, "").split("/")[0] === name)) continue;
      const bindings: Document = {
        property: own(object, name) ?? null, request: invocation.request,
        context: invocation.context, resourceName: invocation.resourceName,
      };
      if (hook !== "onValidate") Object.assign(bindings, { object, propertyName: name });
      const response = await this.evaluate(script, bindings, `property ${hook}`);
      if (hook !== "onValidate") {
        if (isDocument(response.bindings?.object)) {
          replaceDocument(object, response.bindings.object);
        }
        setOwn(object, name, response.valueKind === "undefined" ? null : response.value as JsonValue);
      }
    }
  }
}
