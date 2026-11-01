/** Store, provenance, host errors and nested execution exercised via the JVM. */
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { IdmConfig, IdmEmulator, type Document, type ManagedType } from "../../src/idm/index.ts";
import { HostCallError, RhinoRunner } from "../../src/runner.ts";
import { create, emulator, js, send, simple } from "./fixtures.ts";

let runner: RhinoRunner;
beforeAll(async () => { runner = await RhinoRunner.spawn(); }, 60_000);
afterAll(async () => { await runner?.close(); }, 30_000);

describe("repository and REST seam", () => {
  it("POST generates a UUID, PUT creates chosen ids, revisions bump and stale writes fail", async () => {
    const emu = emulator(runner);
    const generated = await send(emu, { method: "POST", path: "managed/widget", query: { _action: "create" }, body: { text: "first" } }, 201);
    expect(generated._id).toMatch(/^[a-f0-9-]{36}$/);
    const first = await create(emu);
    const second = await send(emu, { method: "PUT", path: "managed/widget/one", headers: { "if-match": String(first._rev) }, body: { text: "b" } });
    expect(second._rev).not.toBe(first._rev);
    await send(emu, { method: "PATCH", path: "managed/widget/one", headers: { "If-Match": String(first._rev) }, body: [{ operation: "replace", field: "/text", value: "c" }] }, 412);
    await send(emu, { method: "DELETE", path: "managed/widget/one", headers: { "If-Match": String(first._rev) } }, 412);
    expect((await send(emu, { method: "DELETE", path: "managed/widget/one" })).text).toBe("b");
    await send(emu, { method: "GET", path: "managed/widget/one" }, 404);
  });

  it("projects id/rev and nulls, handles nested pointers and array patch operations", async () => {
    const emu = emulator(runner);
    await create(emu, { text: "a", nested: { a: 1, b: 2 }, list: ["a", "c"] });
    const body = await send(emu, { method: "GET", path: "managed/widget/one", query: { _fields: "text,other" } });
    expect(body).toEqual({ _id: "one", _rev: "0", text: "a", other: null });
    const patched = await send(emu, { method: "PATCH", path: "managed/widget/one", body: [
      { operation: "replace", field: "/nested/a", value: 3 },
      { operation: "remove", field: "/nested/b" },
      { operation: "add", field: "/list/1", value: "b" },
      { operation: "add", field: "/list/-", value: "d" },
      { operation: "remove", field: "/list/0" },
    ] });
    expect(patched).toMatchObject({ nested: { a: 3 }, list: ["b", "c", "d"] });
    const projected = await send(emu, { method: "GET", path: "managed/widget/one", query: { _fields: "nested/a" } });
    expect(projected).toEqual({ _id: "one", _rev: "1", nested: { a: 3 } });
  });

  it("sorts nested keys, filters, pages and projects query rows", async () => {
    const emu = emulator(runner);
    for (const [id, count] of [["a", 3], ["b", 1], ["c", 2]] as const) await create(emu, { count, nested: { rank: count }, text: id }, id);
    const body = await send(emu, { method: "GET", path: "managed/widget", query: { _queryFilter: "count ge 1", _sortKeys: "-nested/rank,_id", _pageSize: "1", _pagedResultsOffset: "1", _fields: "text" } });
    expect(body).toEqual({ result: [{ _id: "c", _rev: "0", text: "c" }], resultCount: 1, pagedResultsCookie: expect.any(String), remainingPagedResults: -1, totalPagedResults: -1, totalPagedResultsPolicy: "NONE" });
  });

  it("forks share immutable config but isolate nested state and subsequent writes", async () => {
    const input = { managed: { objects: [structuredClone(simple)] } };
    const config = new IdmConfig(input);
    input.managed.objects.length = 0;
    const emu = new IdmEmulator(config, runner);
    await create(emu, { text: "parent", nested: { a: 1 } });
    const child = emu.fork();
    expect(child.config).toBe(emu.config);
    expect(Object.isFrozen(config.managed.objects[0]?.schema.properties)).toBe(true);
    await send(child, { method: "PATCH", path: "managed/widget/one", body: [{ operation: "replace", field: "/nested/a", value: 2 }] });
    expect((await send(emu, { method: "GET", path: "managed/widget/one" })).nested).toEqual({ a: 1 });
    await send(emu, { method: "DELETE", path: "managed/widget/one" });
    expect((await send(child, { method: "GET", path: "managed/widget/one" })).nested).toEqual({ a: 2 });
  });
});

describe("nested hooks and host entry point", () => {
  it("creates through two nested types and runs each type's hooks", async () => {
    const types: ManagedType[] = [
      { name: "root", schema: { properties: { text: { type: "string" } } }, onCreate: js("object.text = openidm.create('managed/branch', 'b', {}).text + '-root';") },
      { name: "branch", schema: { properties: { text: { type: "string" } } }, onCreate: js("object.text = openidm.create('managed/leaf', 'l', {}).text + '-branch';") },
      { name: "leaf", schema: { properties: { text: { type: "string" } } }, onCreate: js("object.text = 'leaf';") },
    ];
    const emu = new IdmEmulator(new IdmConfig({ managed: { objects: types } }), runner);
    const result = await send(emu, { method: "POST", path: "managed/root", query: { _action: "create" }, body: { _id: "r" } }, 201);
    expect(result.text).toBe("leaf-branch-root");
    expect((await send(emu, { method: "GET", path: "managed/branch/b" })).text).toBe("leaf-branch");
    expect((await send(emu, { method: "GET", path: "managed/leaf/l" })).text).toBe("leaf");
  });

  it("script-facing operations use the same store and CREST exception names", async () => {
    const emu = emulator(runner);
    const response = await runner.eval({ engine: "idm", source: `
      var a = openidm.create('managed/widget','one',{text:'a'});
      var b = openidm.update('managed/widget/one',a._rev,{text:'b'});
      var c = openidm.patch('managed/widget/one',b._rev,[{operation:'add',field:'/count',value:3}]);
      var q = openidm.query('managed/widget',{_queryFilter:'count eq 3',_fields:['_id']});
      var record = openidm.read('managed/widget/one',null,['text','count']);
      var error;
      try { openidm.update('managed/widget/one','stale',{text:'c'}); }
      catch(e) { error = {name:e.name,message:e.message,code:e.javaException.getCode()}; }
      openidm.delete('managed/widget/one',null);
      ({record:record,query:q,missing:openidm.read('managed/widget/one'),unrouted:openidm.read('unrouted/path'),error:error});
    ` }, { onCall: emu.onCall });
    expect(response.outcome, JSON.stringify(response.error)).toBe("ok");
    const value = response.value as Document;
    expect(value.record).toMatchObject({ text: "b", count: 3 });
    expect(value.query).toMatchObject({ result: [{ _id: "one", _rev: "2" }], resultCount: 1 });
    expect((value.query as Document).remainingPagedResults).toBeUndefined();
    expect(value).toMatchObject({ missing: null, unrouted: null, error: { name: "JavaException", code: 412 } });
    expect((value.error as Document).message).toContain("org.forgerock.json.resource.PreconditionFailedException");
  });

  it("script create id is separate from hook content and endpoint resourcePath", async () => {
    const type = { ...simple, onCreate: js("object.flag = typeof object._id === 'undefined';") };
    const emu = new IdmEmulator(new IdmConfig({ managed: { objects: [type] }, endpoints: {
      example: js("({path:request.resourcePath,id:request.newResourceId,content:request.content});"),
    } }), runner);
    const response = await runner.eval({ engine: "idm", source: `({
      managed:openidm.create('managed/widget','named',{text:'a'}),
      endpoint:openidm.create('endpoint/example','chosen',{a:1})
    });` }, { onCall: emu.onCall });
    expect(response.outcome).toBe("ok");
    expect(response.value).toMatchObject({ managed: { _id: "named", flag: true }, endpoint: { _id: "chosen", path: "", id: "chosen", content: { a: 1 } } });
  });

  it.each([
    ["openidm.delete('managed/widget/missing',null)", "NotFoundException", 404],
    ["openidm.query('managed/widget',{})", "BadRequestException", 400],
    ["openidm.query('managed/widget',{_queryExpression:'x'})", "NotSupportedException", 405],
    ["openidm.query('managed/widget',{_queryFilter:'true'},['_id'])", "BadRequestException", 400],
  ] as const)("maps %s errors into real JavaException objects", async (source, type, code) => {
    const emu = emulator(runner);
    const response = await runner.eval({ engine: "idm", source: `try {${source};} catch(e) {({code:e.javaException.getCode(),message:e.message,detail:e.javaException.getDetail()});}` }, { onCall: emu.onCall });
    expect(response.outcome).toBe("ok");
    expect(response.value).toMatchObject({ code, detail: {} });
    expect((response.value as Document).message).toContain(`org.forgerock.json.resource.${type}`);
  });
});

describe("provenance and explicit boundaries", () => {
  it("writes report policy omission, reads report measured behavior and a policy seam can replace it", async () => {
    const emu = emulator(runner);
    const write = await emu.request({ method: "POST", path: "managed/widget", query: { _action: "create" }, body: { text: "a" } });
    expect(write.provenance).toContainEqual({ kind: "assumed", behavior: "policy not evaluated" });
    expect(write.provenance.some((entry) => entry.kind === "measured")).toBe(true);
    let checked = 0;
    const withPolicy = new IdmEmulator(emu.config, runner, { policy: ({ object, internal }) => {
      expect(object.text).toBe("checked"); expect(internal).toBe(false); checked++;
    } });
    const result = await withPolicy.request({ method: "POST", path: "managed/widget", query: { _action: "create" }, body: { text: "checked" } });
    expect(result.status).toBe(201);
    expect(checked).toBe(1);
    expect(result.provenance.some((entry) => entry.behavior === "policy not evaluated")).toBe(false);
  });

  it.each(["increment", "move", "copy", "transform"])("unsupported patch %s fails loudly", async (operation) => {
    const emu = emulator(runner);
    await create(emu);
    const result = await emu.request({ method: "PATCH", path: "managed/widget/one", body: [{ operation, field: "/text", value: 1 }] });
    expect(result.status).toBe(501);
    expect(result.body).toMatchObject({ message: `Missing IDM behavior: patch operation ${operation}` });
    expect(result.provenance).toContainEqual({ kind: "missing", behavior: `patch operation ${operation}` });
  });

  it("script file references fail on use with their filename", async () => {
    const emu = emulator(runner, { ...simple, onCreate: { type: "text/javascript", file: "scripts/widget-create.js" } });
    const result = await emu.request({ method: "POST", path: "managed/widget", query: { _action: "create" }, body: {} });
    expect(result.status).toBe(501);
    expect(result.body).toMatchObject({ message: "Missing IDM behavior: script file reference: scripts/widget-create.js" });
  });

  it("missing behavior in a nested call cannot be hidden by a script catch", async () => {
    const type = { ...simple, onCreate: js("try { openidm.action('managed/widget','unknown',{}); } catch(e) {} try { openidm.query('managed/widget',{_queryId:'all'}); } catch(e) {}") };
    const emu = emulator(runner, type);
    const result = await emu.request({ method: "POST", path: "managed/widget", query: { _action: "create" }, body: {} });
    expect(result.status).toBe(501);
    expect(result.provenance).toContainEqual({ kind: "missing", behavior: "named _queryId: all" });
    expect((await send(emu, { method: "GET", path: "managed/widget", query: { _queryFilter: "true" } })).result).toEqual([]);
  });

  it("concurrent request provenance does not leak between jobs", async () => {
    const emu = new IdmEmulator(new IdmConfig({ managed: { objects: [simple] }, endpoints: {
      missing: js("try { openidm.query('managed/widget',{_queryId:'unmeasured'}); } catch(e) {} ({});"),
      normal: js("({ok:true});"),
    } }), runner);
    const [a, b] = await Promise.all([
      emu.request({ method: "GET", path: "endpoint/missing" }),
      emu.request({ method: "GET", path: "endpoint/normal" }),
    ]);
    expect(a.status).toBe(501);
    expect(b.status).toBe(200);
    expect(b.provenance.some((entry) => entry.kind === "missing")).toBe(false);
  });

  it.each(["require('lib/lodash')", "openidm.hash('x','SHA-256')"])("caught unsupported %s still reports missing", async (call) => {
    const emu = new IdmEmulator(new IdmConfig({ managed: { objects: [] }, endpoints: {
      example: js(`try { ${call}; } catch(e) {} ({});`),
    } }), runner);
    const result = await emu.request({ method: "GET", path: "endpoint/example" });
    expect(result.status).toBe(501);
    expect(result.provenance).toContainEqual({ kind: "missing", behavior: `runtime helper: ${call.split("(")[0]}` });
  });

  it("policy seam can reject with CREST detail without storing an object", async () => {
    const emu = new IdmEmulator(new IdmConfig({ managed: { objects: [simple] } }), runner, { policy: () => {
      throw new HostCallError({ class: "org.forgerock.json.resource.ForbiddenException", code: 403,
        message: "Policy validation failed", detail: { failedPolicyRequirements: [{ property: "text" }] } });
    } });
    const result = await emu.request({ method: "POST", path: "managed/widget", query: { _action: "create" }, body: { _id: "one" } });
    expect(result).toMatchObject({ status: 403, body: { message: "Policy validation failed", detail: { failedPolicyRequirements: [{ property: "text" }] } } });
    await send(emu, { method: "GET", path: "managed/widget/one" }, 404);
  });
});
