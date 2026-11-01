import { expectEmulationFailure } from "../../src/harness/emulation-guard.ts";
/** Reviewer regressions through the real emulator and JVM. */
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { RhinoRunner } from "../../src/runner.ts";
import { IdmConfig, IdmEmulator, type Document } from "../../src/idm/index.ts";
import { create, emulator, js, send, simple } from "./fixtures.ts";

let runner: RhinoRunner;
beforeAll(async () => { runner = await RhinoRunner.spawn(); }, 60_000);
afterAll(async () => { await runner?.close(); }, 30_000);

describe("review round 1 core", () => {
  it.each(["create", "update"] as const)("#2 %s keeps object and newObject aliased", async (method) => {
    const script = js("newObject.text='via-new'; object.flag=object===newObject;");
    const emu = emulator(runner, { ...simple, [method === "create" ? "onCreate" : "onUpdate"]: script });
    await create(emu);
    if (method === "update") await send(emu, { method: "PATCH", path: "managed/widget/one", body: [{ operation: "replace", field: "/text", value: "update" }] });
    expect(await send(emu, { method: "GET", path: "managed/widget/one" })).toMatchObject({ text: "via-new", flag: true });
  });

  it("#3 applies declared defaults before policy on create only", async () => {
    const seen: Document[] = [];
    const type = { name: "widget", schema: { properties: {
      text: { type: "string", default: "hello" }, count: { type: "number", default: 7 },
      zero: { type: "number", default: 0 }, flag: { type: "boolean", default: false },
      yes: { type: "boolean", default: true }, list: { type: "array", default: ["a", "b"] },
      empty: { type: "array", default: [] },
    }, required: ["count"] } };
    const emu = new IdmEmulator(new IdmConfig({ managed: { objects: [type] } }), runner, {
      policy: ({ object }) => { seen.push(structuredClone(object)); },
    });
    const first = await create(emu, {});
    const defaults = { text: "hello", count: 7, zero: 0, flag: false, yes: true, list: ["a", "b"], empty: [] };
    expect(seen[0]).toEqual(defaults);
    expect(first).toMatchObject(defaults);
    await create(emu, { count: null, flag: true, list: ["explicit"] }, "two");
    expect(seen[1]).toMatchObject({ count: null, flag: true, list: ["explicit"] });
    await send(emu, { method: "PUT", path: "managed/widget/one", headers: { "If-Match": "0" }, body: {} });
    expect(seen[2]).toEqual({});
    expect((await send(emu, { method: "GET", path: "managed/widget/two" })).list).toEqual(["explicit"]);
  });

  it("#4 opaque cookies walk every row with measured REST and script envelopes", async () => {
    const emu = emulator(runner);
    for (const id of ["a", "b", "c"]) await create(emu, { text: id }, id);
    const ids: string[] = [];
    let cookie: string | undefined;
    do {
      const page = await send(emu, { method: "GET", path: "managed/widget", query: {
        _queryFilter: "true", _pageSize: 1, _sortKeys: "_id", ...(cookie ? { _pagedResultsCookie: cookie } : {}),
      } });
      expect(page).toMatchObject({ remainingPagedResults: -1, totalPagedResults: -1, totalPagedResultsPolicy: "NONE" });
      expect(page.resultCount).toBe((page.result as Document[]).length);
      ids.push(...(page.result as Document[]).map((row) => String(row._id)));
      cookie = typeof page.pagedResultsCookie === "string" && page.pagedResultsCookie ? page.pagedResultsCookie : undefined;
    } while (cookie && ids.length < 10);
    expect(ids).toEqual(["a", "b", "c"]);
    const response = await runner.eval({ engine: "idm", source: `var ids=[],cookie; do {
      var q=openidm.query('managed/widget',{_queryFilter:'true',_pageSize:1,_pagedResultsCookie:cookie||''});
      ids.push(q.result[0]._id); cookie=q.pagedResultsCookie;
    } while(cookie && ids.length<10); ({ids:ids,remaining:q.remainingPagedResults,total:q.totalPagedResults,policy:q.totalPagedResultsPolicy});` }, { onCall: emu.onCall });
    expect(response.value).toEqual({ ids: ["a", "b", "c"], remaining: null, total: -1, policy: "NONE" });
    await send(emu, { method: "GET", path: "managed/widget", query: { _queryFilter: "true", _pagedResultsCookie: null } }, 500);
  });

  it.each(["__proto__", "constructor", "prototype"])("#5 treats %s as pointer data without inherited traversal", async (key) => {
    const emu = emulator(runner);
    await create(emu, { nested: {} });
    const child = emu.fork();
    try {
      const rejected = await child.request({ method: "PATCH", path: "managed/widget/one", body: [{ operation: "add", field: `/nested/${key}/round1Marker`, value: "polluted" }] });
      expect(rejected.status).toBe(400);
      expect(Object.hasOwn(Object.prototype, "round1Marker")).toBe(false);
      const projection = await send(child, { method: "GET", path: "managed/widget/one", query: { _fields: `nested/${key}/round1Marker` } });
      expect(Object.hasOwn(Object.prototype, "round1Marker")).toBe(false);
      expect(projection.nested).toEqual(JSON.parse(`{"${key}":{"round1Marker":null}}`));
      await send(child, { method: "PATCH", path: "managed/widget/one", body: [
        { operation: "add", field: `/nested/${key}`, value: {} },
        { operation: "add", field: `/nested/${key}/round1Marker`, value: "data" },
      ] });
      expect((await send(child, { method: "GET", path: "managed/widget/one", query: { _fields: `nested/${key}/round1Marker` } })).nested).toEqual(JSON.parse(`{"${key}":{"round1Marker":"data"}}`));
      expect((await send(emu, { method: "GET", path: "managed/widget/one" })).nested).toEqual({});
    } finally { Reflect.deleteProperty(Object.prototype, "round1Marker"); }
  });

  it("#7 rechecks a conditional DELETE after its hook yields", async () => {
    let release!: () => void;
    let entered!: () => void;
    const waiting = new Promise<void>((resolve) => { entered = resolve; });
    const gate = new Promise<void>((resolve) => { release = resolve; });
    const emu = new IdmEmulator(new IdmConfig({ managed: { objects: [
      { ...simple, onDelete: js("openidm.create('managed/gate','gate',{});") },
      { name: "gate", schema: { properties: {} } },
    ] } }), runner, { policy: async ({ schema }) => {
      if (Object.keys(schema.properties).length === 0) { entered(); await gate; }
    } });
    await create(emu);
    const deletion = emu.request({ method: "DELETE", path: "managed/widget/one", headers: { "If-Match": "0" } });
    await waiting;
    try {
      await send(emu, { method: "PATCH", path: "managed/widget/one", headers: { "If-Match": "0" }, body: [{ operation: "replace", field: "/text", value: "new" }] });
    } finally { release(); }
    expect((await deletion).status).toBe(412);
    expect(await send(emu, { method: "GET", path: "managed/widget/one" })).toMatchObject({ text: "new", _rev: "1" });
  });

  it.each(["postCreate", "postUpdate", "onSync"] as const)("#13 labels only an actual %s failure after commit", async (hook) => {
    const emu = emulator(runner, { ...simple, [hook]: js("if(object===null || object.text==='fail') { try { for(var p in this) {} } catch(e) {} }") });
    if (hook !== "onSync") {
      const ok = await emu.request({ method: "PUT", path: "managed/widget/ok", body: { text: "ok" } });
      expect(ok.provenance.some((p) => p.behavior === "post-hook failure after commit keeps the write")).toBe(false);
    }
    if (hook === "postUpdate") await create(emu);
    const result = await emu.request({ method: hook === "postUpdate" ? "PATCH" : "PUT", path: "managed/widget/one", body: hook === "postUpdate" ? [{ operation: "replace", field: "/text", value: "fail" }] : { text: "fail" } });
    expect(result.status).toBe(500);
    expect(result.provenance).toContainEqual({ kind: "assumed", behavior: "post-hook failure after commit keeps the write" });
    expect((await send(emu, { method: "GET", path: "managed/widget/one" })).text).toBe("fail");
  });
});


describe("review round 1 caller provenance", () => {
  it("#1 standalone onCall cannot swallow missing and jobs remain isolated", async () => {
    expectEmulationFailure("named _queryId: unmeasured");
    const emu = emulator(runner);
    const source = "try { openidm.query('managed/widget',{_queryId:'unmeasured'}); } catch(e) {} 'passed'";
    const [caught, clean] = await Promise.all([
      runner.eval({ engine: "idm", source, reportMissing: true }, { onCall: emu.onCall }),
      runner.eval({ engine: "idm", source: "'clean'", reportMissing: true }, { onCall: emu.onCall }),
    ]);
    expect(caught.outcome).not.toBe("ok");
    expect(caught.missing).toContain("named _queryId: unmeasured");
    expect(clean.missing).toEqual([]);
    const noOptIn = await runner.eval({ engine: "idm", source }, { onCall: emu.onCall });
    expect(noOptIn.outcome).not.toBe("ok");
    expect(noOptIn.missing).toContain("named _queryId: unmeasured");
  });

  it("#9 properties belong to the emulator, forks inherit them, absent names carry provenance", async () => {
    const config = new IdmConfig({ managed: { objects: [] }, endpoints: {
      example: js("({present:identityServer.getProperty('esv.feature','off'),absent:identityServer.getProperty('esv.absent',{fallback:true})});"),
    } });
    const properties = { "esv.feature": "on" };
    const emu = new IdmEmulator(config, runner, { properties });
    properties["esv.feature"] = "changed";
    for (const instance of [emu, emu.fork()]) {
      const result = await instance.request({ method: "GET", path: "endpoint/example" });
      expect(result.body).toMatchObject({ present: "on", absent: { fallback: true } });
      expect(result.provenance).toContainEqual({ kind: "assumed", behavior: "property esv.absent absent" });
      expect(result.provenance.some((p) => p.behavior === "property esv.feature absent")).toBe(false);
    }
    const separate = await new IdmEmulator(config, runner).request({ method: "GET", path: "endpoint/example" });
    expect(separate.body).toMatchObject({ present: "off" });
    expect(separate.provenance).toContainEqual({ kind: "assumed", behavior: "property esv.feature absent" });
  });
});


it.each(["__proto__", "constructor", "prototype"])("#5 stores a literal top-level %s field through a JVM hook", async (key) => {
  const emu = emulator(runner, { ...simple,
    schema: { properties: { ...simple.schema.properties, [key]: { type: "object" } } },
    onCreate: js("object.text='hook ran';"),
  });
  const body = JSON.parse(`{"${key}":{"round1Marker":"data"}}`) as Document;
  const created = await create(emu, body);
  expect(Object.hasOwn(created, key)).toBe(true);
  expect(created[key]).toEqual({ round1Marker: "data" });
  const read = await send(emu, { method: "GET", path: "managed/widget/one" });
  expect(read[key]).toEqual({ round1Marker: "data" });
  expect(Object.hasOwn(Object.prototype, "round1Marker")).toBe(false);
  const absent = await create(emu, {}, "empty");
  expect(Object.hasOwn(absent, key)).toBe(true);
  expect(absent[key]).toBeNull();
});


it("#7 compares the stored revision snapshot even if onDelete mutates its object", async () => {
  const emu = emulator(runner, { ...simple, onDelete: js("object._rev='hook-local';") });
  await create(emu);
  const result = await emu.request({ method: "DELETE", path: "managed/widget/one", headers: { "If-Match": "0" } });
  expect(result.status).toBe(200);
  await send(emu, { method: "GET", path: "managed/widget/one" }, 404);
});
