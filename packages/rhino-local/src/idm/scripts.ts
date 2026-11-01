/** Hook binding graphs and IDM evaluations. Host calls re-enter the owning emulator. */
import { AsyncLocalStorage } from "node:async_hooks";
import type { EvalOptions, RhinoRunner } from "../runner.ts";
import type { JsonValue, JobResponse } from "../protocol.ts";
import { isDocument, replaceDocument } from "./document.ts";
import { missing, scriptError } from "./errors.ts";
import type { Evidence } from "./provenance.ts";
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
  private readonly onCall: NonNullable<EvalOptions["onCall"]>;
  constructor(
    runner: RhinoRunner,
    evidence: Evidence,
    onCall: NonNullable<EvalOptions["onCall"]>,
    properties: Record<string, JsonValue>,
  ) {
    this.runner = runner;
    this.evidence = evidence;
    this.onCall = onCall;
    this.environmentProperties = properties;
  }

  async evaluate(script: Script, bindings: Document, context: string, bindingAliases?: Record<string, string>): Promise<JobResponse> {
    if (script.file !== undefined) missing(this.evidence, `script file reference: ${script.file}`);
    if (script.type !== "text/javascript") missing(this.evidence, `script type: ${script.type}`);
    if (script.source === undefined) missing(this.evidence, "script without inline source");
    this.evidence.measured("scripts execute on the IDM engine with resource-name binding");
    this.evidence.measured("request uses a CREST method request wrapper");
    this.evidence.assumed("Java long values use JavaScript number precision");
    const response = await this.runner.eval({
      ...(bindingAliases ? { bindingAliases } : {}),
      properties: this.environmentProperties,
      engine: "idm", source: script.source, sourceName: context,
      bindings: { ...script.globals, ...bindings }, resourceNameBindings: ["resourceName"], reportMissing: true, crestBindings: true,
    }, { onCall: AsyncLocalStorage.bind(this.onCall) });
    for (const assumption of response.assumed ?? []) this.evidence.assumed(assumption);
    for (const helper of response.missing ?? []) this.evidence.add("missing", `runtime helper: ${helper}`);
    const absent = this.evidence.firstMissing();
    if (absent) missing(this.evidence, absent.behavior);
    if (response.outcome !== "ok") scriptError(response, context, this.evidence);
    return response;
  }

  async hook(type: ManagedType, hook: Hook, invocation: Invocation): Promise<void> {
    const script = type[hook];
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
      const script = config[hook];
      if (!script) continue;
      if (hook === "onRetrieve" && selectors.length > 0 && !selectors.includes("*") &&
        !selectors.some((field) => field.replace(/^\//, "").split("/")[0] === name)) continue;
      const bindings: Document = {
        property: object[name] ?? null, request: invocation.request,
        context: invocation.context, resourceName: invocation.resourceName,
      };
      if (hook !== "onValidate") Object.assign(bindings, { object, propertyName: name });
      const response = await this.evaluate(script, bindings, `property ${hook}`);
      if (hook !== "onValidate") {
        if (isDocument(response.bindings?.object)) {
          replaceDocument(object, response.bindings.object);
        }
        object[name] = response.valueKind === "undefined" ? null : response.value as JsonValue;
      }
    }
  }
}
