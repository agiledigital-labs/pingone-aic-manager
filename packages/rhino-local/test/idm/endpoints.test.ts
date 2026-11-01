/** Real IDM scripts discriminate request shapes, completion mapping and throw contexts. */
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { IdmConfig, IdmEmulator, type Document, type RestRequest } from "../../src/idm/index.ts";
import { RhinoRunner } from "../../src/runner.ts";
import { emulator, js, send, simple } from "./fixtures.ts";

let runner: RhinoRunner;
beforeAll(async () => { runner = await RhinoRunner.spawn(); }, 60_000);
afterAll(async () => { await runner?.close(); }, 30_000);

function endpoint(source: string): IdmEmulator {
  return new IdmEmulator(new IdmConfig({ managed: { objects: [simple] }, endpoints: { example: js(source) } }), runner);
}

describe("endpoint request bindings", () => {
  const echo = `({method:request.method,keys:Object.keys(request).sort(),path:request.resourcePath,
    resource:String(resourceName),resourceType:typeof resourceName,objectType:typeof object,
    parameters:context.http.parameters,content:request.content === undefined ? null : request.content,
    newResourceId:request.newResourceId === undefined ? null : request.newResourceId,
    fields:request.fields,additional:request.additionalParameters,revision:request.revision === undefined ? null : request.revision,
    patch:request.patchOperations === undefined ? null : request.patchOperations});`;
  const common = ["method", "fields", "resourcePath", "additionalParameters"];
  const cases: { name: string; request: RestRequest; method: string; path: string; keys: string[]; status?: number }[] = [
    { name: "GET root", request: { method: "GET", path: "endpoint/example" }, method: "read", path: "", keys: [] },
    { name: "GET subpath", request: { method: "GET", path: "endpoint/example/a/b" }, method: "read", path: "a/b", keys: [] },
    { name: "POST action", request: { method: "POST", path: "endpoint/example", query: { _action: "go" }, body: { a: 1 } }, method: "action", path: "", keys: ["action", "content"] },
    { name: "POST create", request: { method: "POST", path: "endpoint/example", body: { a: 1 } }, method: "create", path: "", keys: ["content", "newResourceId"], status: 201 },
    { name: "PUT create", request: { method: "PUT", path: "endpoint/example/id1", body: { a: 1 } }, method: "create", path: "", keys: ["content", "newResourceId"], status: 201 },
    { name: "PUT update", request: { method: "PUT", path: "endpoint/example/id1", headers: { "If-Match": "*" }, body: { a: 1 } }, method: "update", path: "id1", keys: ["content", "revision"] },
    { name: "PATCH", request: { method: "PATCH", path: "endpoint/example/id1", body: [{ operation: "add", field: "/a", value: 1 }] }, method: "patch", path: "id1", keys: ["patchOperations", "revision"] },
    { name: "DELETE", request: { method: "DELETE", path: "endpoint/example/id1" }, method: "delete", path: "id1", keys: ["revision"] },
  ];
  it.each(cases)("$name", async ({ request, method, path, keys, status }) => {
    const body = await send(endpoint(echo), request, status ?? 200);
    expect(body).toMatchObject({ method, keys: [...common, ...keys].sort(), path, resource: path, resourceType: "object", objectType: "undefined", fields: [] });
    if (request.method === "PUT" && method === "create") expect(body.newResourceId).toBe("id1");
    if (method === "action") expect(body._id).toBeUndefined();
    else expect(body._id).toBe(request.path.split("/").slice(2).at(-1) ?? "");
  });

  it("binds fields, extra parameters and inherited root HTTP context in nested calls", async () => {
    const emu = new IdmEmulator(new IdmConfig({ managed: { objects: [simple] }, endpoints: {
      outer: js("openidm.action('endpoint/inner','go',{},{q:'inner'});"),
      inner: js("({path:context.http.path,method:context.http.method,root:context.http.parameters.q,extra:request.additionalParameters.q});"),
    } }), runner);
    const body = await send(emu, { method: "GET", path: "endpoint/outer", query: { q: "root" } });
    expect(body).toMatchObject({ path: "endpoint/outer", method: "GET", root: "root", extra: "inner" });
  });

  it("supports nested scripted config and globals", async () => {
    const emu = new IdmEmulator(new IdmConfig({ managed: { objects: [] }, endpoints: {
      example: { type: "scripted", script: { ...js("({answer:configValue});"), globals: { configValue: 42 } } },
    } }), runner);
    expect(await send(emu, { method: "GET", path: "endpoint/example" })).toEqual({ answer: 42, _id: "" });
  });

  it("HTTP header lookup ignores case while keys keep their supplied spelling", async () => {
    const emu = endpoint("({a:context.http.headers['X-EXAMPLE'],b:context.http.headers['x-example'],keys:Object.keys(context.http.headers),tag:Object.prototype.toString.call(request)});");
    const body = await send(emu, { method: "GET", path: "endpoint/example", headers: { "X-Example": "one", "x-example": "two" } });
    expect(body).toEqual({ a: "one,two", b: "one,two", keys: ["X-Example"], tag: "[object ScriptableReadRequest]", _id: "" });
  });
});

describe("endpoint completion mapping", () => {
  it.each(["null", "undefined"])("%s read is 404", async (source) => {
    const body = await send(endpoint(`${source};`), { method: "GET", path: "endpoint/example" }, 404);
    expect(body).toEqual({ code: 404, reason: "Not Found", message: "script returned null" });
  });

  it("null action is 200 with null body and no id stamping", async () => {
    const result = await endpoint("null;").request({ method: "POST", path: "endpoint/example", query: { _action: "go" } });
    expect(result).toMatchObject({ status: 200, body: null });
  });

  it.each(["[]", "'text'", "42"])("%s read preserves the measured truncated bytes", async (source) => {
    const result = await endpoint(`${source};`).request({ method: "GET", path: "endpoint/example" });
    expect(result).toMatchObject({ status: 200, body: null, rawBody: '{"_id":""' });
    expect(() => JSON.parse(result.rawBody ?? "")).toThrow();
    expect(result.provenance).toContainEqual({ kind: "measured", behavior: "endpoint scalar/array read returns truncated invalid JSON bytes", source: "docs/api/24-idm-script-runtime.md" });
  });

  it("array query wraps rows and derives _id from id", async () => {
    const body = await send(endpoint("[{id:'first',v:1},{_id:'second',id:'kept'}];"), { method: "GET", path: "endpoint/example", query: { _queryFilter: "true" } });
    expect(body).toMatchObject({ result: [{ id: "first", _id: "first", v: 1 }, { _id: "second", id: "kept" }], resultCount: 2 });
  });

  it("null query is empty; object query is the measured 500", async () => {
    const request: RestRequest = { method: "GET", path: "endpoint/example", query: { _queryFilter: "true" } };
    expect((await send(endpoint("null;"), request)).result).toEqual([]);
    expect((await send(endpoint("({});"), request, 500)).message).toBe("Script returned unexpected query result structure of type class java.util.HashMap");
  });

  it("nested Date, NaN, Infinity and object ordering use the runner's Java conversion", async () => {
    const body = await send(endpoint("({date:new Date(0),nan:NaN,infinity:Infinity,ordered:{b:1,a:2}});"), { method: "GET", path: "endpoint/example" });
    expect(body).toMatchObject({ date: {}, nan: "NaN", infinity: Number("9223372036854775807"), ordered: { a: 2, b: 1 } });
    expect(Object.keys(body.ordered as Document)).toEqual(["a", "b"]);
  });
});

describe("per-context throws", () => {
  it.each([
    ["throw {code:400}", 400, { code: 400, reason: "Bad Request", message: "[object Object]", detail: {} }],
    ["throw {code:409,message:'conflict',detail:{k:1}}", 409, { code: 409, reason: "Conflict", message: "conflict", detail: { k: 1 } }],
    ["throw new Error('x')", 500, { code: 500, reason: "Internal Server Error", message: "Error: x" }],
    ["throw 'x'", 500, { code: 500, reason: "Internal Server Error", message: "x" }],
  ])("endpoint %s", async (source, status, expected) => {
    expect(await send(endpoint(String(source)), { method: "GET", path: "endpoint/example" }, Number(status))).toEqual(expected);
  });

  it("string-valued code yields 500 and an empty body", async () => {
    expect(await endpoint("throw {code:'404'};").request({ method: "GET", path: "endpoint/example" })).toMatchObject({ status: 500, body: null, rawBody: "" });
  });

  it("onCreate numeric fault retains detail and prevents storage; Error is generic 500", async () => {
    for (const [source, code, message] of [
      ["throw {code:418,message:'tea',detail:{a:1}};", 418, "tea"],
      ["throw new Error('plain error');", 500, "Internal Server Error"],
    ] as const) {
      const emu = emulator(runner, { ...simple, onCreate: js(source), postCreate: js("throw {code:409};") });
      const body = await send(emu, { method: "POST", path: "managed/widget", query: { _action: "create" }, body: { _id: "one" } }, code);
      expect(body.message).toBe(message);
      if (code === 418) expect(body).toEqual({ code, reason: "Resource Exception", message, detail: { a: 1 } });
      await send(emu, { method: "GET", path: "managed/widget/one" }, 404);
    }
  });

  it.each(["throw 'bad pval';", "unknownBinding.member;"])("property onValidate %s is 403", async (source) => {
    const emu = emulator(runner, { ...simple, schema: { properties: { ...simple.schema.properties, text: { type: "string", onValidate: js(source) } } } });
    const body = await send(emu, { method: "POST", path: "managed/widget", query: { _action: "create" }, body: { text: "a" } }, 403);
    if (source.startsWith("throw")) expect(body.message).toBe("bad pval");
    else expect(body.message).toContain("javax.script.ScriptException: ReferenceError:");
  });

  it("unmeasured onStore Error mapping is marked assumed", async () => {
    const emu = emulator(runner, { ...simple, onStore: js("throw new Error('unmeasured');") });
    const result = await emu.request({ method: "POST", path: "managed/widget", query: { _action: "create" }, body: {} });
    expect(result.status).toBe(500);
    expect(result.provenance).toContainEqual({ kind: "assumed", behavior: "onStore: unmeasured throw yields generic 500" });
  });

  it("endpoint fault strings are HTML escaped recursively at the HTTP seam", async () => {
    const body = await send(endpoint("throw {code:400,message:'must be <= 100',detail:{nested:[\"a=b@c\",\"'quoted'\"]}};"), { method: "GET", path: "endpoint/example" }, 400);
    expect(body).toMatchObject({ message: "must be &lt;&#61; 100", detail: { nested: ["a&#61;b&#64;c", "&#39;quoted&#39;"] } });
  });
});
