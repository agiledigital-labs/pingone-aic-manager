/** Exact sequences from doc 24, emitted by hooks through nested host calls. */
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { RhinoRunner } from "../../src/runner.ts";
import { clearTrace, create, send, trace, traced } from "./fixtures.ts";
import type { Document, RestRequest } from "../../src/idm/index.ts";

let runner: RhinoRunner;
beforeAll(async () => { runner = await RhinoRunner.spawn(); }, 60_000);
afterAll(async () => { await runner?.close(); }, 30_000);

const createChain = ["onCreate", "property.onValidate", "onValidate", "property.onStore", "property.onStore.none", "onStore"];
const readChain = ["onRetrieve", "property.onRetrieve", "onRead"];
const updateChain = ["onUpdate", "property.onValidate", "onValidate", "property.onStore", "property.onStore.none", "onStore", "postUpdate", "onSync"];

describe("measured lifecycle order", () => {
  it("create has the complete create, property, post and sync chain", async () => {
    const emu = traced(runner);
    const body = await create(emu, { text: "a", pstore: "a" });
    expect((await trace(emu)).map((row) => row.phase)).toEqual([...createChain, "postCreate", "onSync"]);
    expect(body).toMatchObject({ pstore: "a-stored", pnone: null, created: 1 });
  });

  const cases: { name: string; request: RestRequest; phases: string[]; status?: number }[] = [
    { name: "read", request: { method: "GET", path: "managed/widget/one" }, phases: readChain },
    { name: "_fields read", request: { method: "GET", path: "managed/widget/one", query: { _fields: "text" } }, phases: ["onRetrieve", "onRead"] },
    { name: "query", request: { method: "GET", path: "managed/widget", query: { _queryFilter: "true" } }, phases: [] },
    { name: "query executeOnRetrieve", request: { method: "GET", path: "managed/widget", query: { _queryFilter: "true", executeOnRetrieve: "true" } }, phases: ["onRetrieve", "property.onRetrieve"] },
    { name: "PATCH", request: { method: "PATCH", path: "managed/widget/one", body: [{ operation: "replace", field: "/text", value: "b" }] }, phases: [...readChain, ...updateChain] },
    { name: "no-change PATCH", request: { method: "PATCH", path: "managed/widget/one", body: [{ operation: "replace", field: "/text", value: "a" }] }, phases: readChain },
    { name: "If-Match PUT", request: { method: "PUT", path: "managed/widget/one", headers: { "If-Match": "0" }, body: { text: "b" } }, phases: [...readChain, ...updateChain] },
    { name: "body _rev PUT", request: { method: "PUT", path: "managed/widget/one", body: { text: "b", _rev: "0" } }, phases: [...readChain, ...updateChain] },
    { name: "unconditional existing PUT", request: { method: "PUT", path: "managed/widget/one", body: { text: "b" } }, phases: [...createChain, ...updateChain] },
    { name: "duplicate If-None-Match create", request: { method: "PUT", path: "managed/widget/one", headers: { "If-None-Match": "*" }, body: { text: "b" } }, phases: createChain, status: 412 },
    { name: "duplicate POST create", request: { method: "POST", path: "managed/widget", query: { _action: "create" }, body: { _id: "one", text: "b" } }, phases: createChain, status: 412 },
    { name: "delete", request: { method: "DELETE", path: "managed/widget/one" }, phases: ["onDelete", "postDelete", "onSync"] },
    { name: "object action", request: { method: "POST", path: "managed/widget/one", query: { _action: "ping" }, body: {} }, phases: [...readChain, "action", ...readChain] },
  ];
  it.each(cases)("$name", async ({ request, phases, status }) => {
    const emu = traced(runner);
    await create(emu);
    await clearTrace(emu);
    const result = await send(emu, request, status ?? 200);
    expect((await trace(emu)).map((row) => row.phase)).toEqual(phases);
    if (request.method === "PATCH" && phases.length > 3) {
      expect(result.pstore).toBe("null-stored-stored");
      expect(result.readMutation).toBeNull();
    }
    if (request.method === "PUT" && !request.headers && !(request.body as Document)?._rev) {
      expect(result.created).toBeNull();
    }
    if (request.method === "POST" && status !== 412) {
      expect(result.discarded).toBeUndefined();
      expect(result.readMutation).toBe("read");
    }
  });

  it("property onStore re-applies on an unrelated write and runs when absent", async () => {
    const emu = traced(runner);
    expect((await create(emu, { text: "a", pstore: "a" })).pstore).toBe("a-stored");
    const patched = await send(emu, { method: "PATCH", path: "managed/widget/one", body: [{ operation: "add", field: "/other", value: "b" }] });
    expect(patched.pstore).toBe("a-stored-stored");
    expect((await create(emu, { text: "a" }, "two")).pstore).toBe("null-stored");
    expect(patched.pnone).toBeNull();
  });

  it("create bindings include measured absences, nulls and Java resource names", async () => {
    const emu = traced(runner);
    await create(emu);
    const rows = await trace(emu);
    const snapshot = (phase: string) => rows.find((row) => row.phase === phase)?.snapshot as Document;
    expect(snapshot("onCreate")).toMatchObject({ objectType: "object", objectTag: "[object ScriptableMap]", oldNull: true, newType: "object", method: "create", resource: "managed/widget/one", resourceTag: "[object JavaObject]", contextType: "object" });
    expect(snapshot("onCreate").requestTag).toBe("[object ScriptableCreateRequest]");
    expect(snapshot("property.onValidate")).toMatchObject({ objectType: "undefined", propertyType: "object", property: null, propertyNameType: "undefined", valueType: "undefined", oldType: "undefined", newType: "undefined" });
    expect(snapshot("property.onStore")).toMatchObject({ objectType: "object", propertyNameType: "string", valueType: "undefined" });
    expect(snapshot("onStore")).toMatchObject({ objectType: "object", valueType: "object", oldType: "undefined", newType: "undefined" });
    expect(snapshot("onSync")).toMatchObject({ objectNull: true, oldNull: true, newType: "object" });
  });

  it("delete bindings give postDelete and onSync null object and newObject", async () => {
    const emu = traced(runner);
    await create(emu);
    await clearTrace(emu);
    await send(emu, { method: "DELETE", path: "managed/widget/one" });
    const rows = await trace(emu);
    expect(rows[0]?.snapshot).toMatchObject({ objectNull: false, oldNull: false, newNull: true });
    for (const row of rows.slice(1)) expect(row.snapshot).toMatchObject({ objectNull: true, oldNull: false, newNull: true });
  });

  it("collection custom action is the measured 400", async () => {
    const body = await send(traced(runner), { method: "POST", path: "managed/widget", query: { _action: "ping" } }, 400);
    expect(body).toEqual({ code: 400, reason: "Bad Request", message: "Expecting String containing one of: patch triggerSyncCheck updateLastSync" });
  });

  it("executeOnRetrieve runs per row without onRead", async () => {
    const emu = traced(runner);
    await create(emu);
    await create(emu, { text: "b" }, "two");
    await clearTrace(emu);
    await send(emu, { method: "GET", path: "managed/widget", query: { _queryFilter: "true", executeOnRetrieve: true } });
    expect((await trace(emu)).map((row) => row.phase)).toEqual(["onRetrieve", "property.onRetrieve", "onRetrieve", "property.onRetrieve"]);
  });
});
