/** Invented configurations and trace scripts; every rule runs on the real JVM. */
import { expect } from "vitest";
import { IdmConfig, IdmEmulator, type Document, type ManagedType, type RestRequest, type Script } from "../../src/idm/index.ts";
import type { RhinoRunner } from "../../src/runner.ts";

export const js = (source: string): Script => ({ type: "text/javascript", source });
export const simple: ManagedType = { name: "widget", schema: { properties: {
  text: { type: "string" }, count: { type: "number" }, flag: { type: "boolean" },
  nested: { type: "object" }, list: { type: "array" }, other: { type: "string" },
  spaced: { type: "string" }, label: { type: "string" }, nulls: { type: "array" }, word: { type: "string" },
} } };

export function emulator(runner: RhinoRunner, type: ManagedType = simple): IdmEmulator {
  return new IdmEmulator(new IdmConfig({ sync: { mappings: [] }, managed: { _id: "managed", objects: [type] } }), runner);
}

export function traceScript(phase: string, tail = ""): Script {
  return js(`(function () { openidm.create('managed/trace', null, {
    phase: ${JSON.stringify(phase)},
    snapshot: {
      objectType: typeof object, objectNull: typeof object !== 'undefined' && object === null,
      objectTag: typeof object !== 'undefined' && object !== null ? Object.prototype.toString.call(object) : null,
      objectText: typeof object !== 'undefined' && object !== null ? object.text : null,
      oldType: typeof oldObject, oldNull: typeof oldObject !== 'undefined' && oldObject === null,
      oldRev: typeof oldObject !== 'undefined' && oldObject !== null ? oldObject._rev : null,
      newType: typeof newObject, newNull: typeof newObject !== 'undefined' && newObject === null,
      propertyType: typeof property, property: typeof property !== 'undefined' ? property : null,
      propertyNameType: typeof propertyName, valueType: typeof value,
      method: request.method, resource: String(resourceName), resourceTag: Object.prototype.toString.call(resourceName),
      contextType: typeof context, requestTag: Object.prototype.toString.call(request)
    }
  }); })(); ${tail}`);
}

export const traceType: ManagedType = { name: "trace", schema: { properties: {
  phase: { type: "string" }, snapshot: { type: "object" },
} } };

export function traced(runner: RhinoRunner): IdmEmulator {
  const type: ManagedType = {
    name: "widget",
    schema: { properties: {
      text: { type: "string" }, other: { type: "string" },
      pval: { type: "string", onValidate: traceScript("property.onValidate") },
      pstore: { type: "string", onStore: traceScript("property.onStore", 'property + "-stored";') },
      pnone: { type: "string", onStore: traceScript("property.onStore.none", "var ignored = 1;") },
      pret: { type: "string", onRetrieve: traceScript("property.onRetrieve", 'property + "-retrieved";') },
      created: { type: "number" }, readMutation: { type: "string" },
    } },
    onCreate: traceScript("onCreate", "object.created = (object.created || 0) + 1;"),
    onUpdate: traceScript("onUpdate"),
    onValidate: traceScript("onValidate"),
    onStore: traceScript("onStore"),
    postCreate: traceScript("postCreate"),
    postUpdate: traceScript("postUpdate"),
    onRetrieve: traceScript("onRetrieve", "object.readMutation = 'retrieve';"),
    onRead: traceScript("onRead", "object.readMutation = 'read';"),
    onDelete: traceScript("onDelete"),
    postDelete: traceScript("postDelete"),
    onSync: traceScript("onSync"),
    actions: { ping: traceScript("action", "({ discarded: true });") },
  };
  return new IdmEmulator(new IdmConfig({ sync: { mappings: [] }, managed: { objects: [type, traceType] } }), runner);
}

export async function send(emu: IdmEmulator, request: RestRequest, status = 200): Promise<Document> {
  const result = await emu.request(request);
  expect(result.status, JSON.stringify(result)).toBe(status);
  return result.body as Document;
}

export async function create(emu: IdmEmulator, body: Document = { text: "a" }, id = "one"): Promise<Document> {
  return send(emu, { method: "PUT", path: `managed/widget/${id}`, headers: { "If-None-Match": "*" }, body }, 201);
}

export async function trace(emu: IdmEmulator): Promise<Document[]> {
  const result = await send(emu, { method: "GET", path: "managed/trace", query: { _queryFilter: "true" } });
  return result.result as Document[];
}

export async function clearTrace(emu: IdmEmulator): Promise<void> {
  for (const row of await trace(emu)) await send(emu, { method: "DELETE", path: `managed/trace/${String(row._id)}` });
}
