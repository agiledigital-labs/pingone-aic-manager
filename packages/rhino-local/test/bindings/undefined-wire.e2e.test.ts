/**
 * docs/api/12 (2026-11-04) on the real JVM: an undefined openidm.create content field is sent as
 * null, and an httpClient.send body sends undefined as null at any depth, the body included.
 */
import { afterAll, beforeAll, expect, it } from "vitest";
import { runCase } from "../../src/bindings/index.ts";
import { defineCase } from "../../src/case/index.ts";
import { expectHarnessFault } from "../../src/harness/emulation-guard.ts";
import { RhinoRunner } from "../../src/runner.ts";

let runner: RhinoRunner;
beforeAll(async () => {
  runner = await RhinoRunner.spawn();
}, 60_000);
afterAll(async () => {
  await runner?.close();
}, 30_000);

const run = async (script: string) =>
  (
    await runCase(
      runner,
      defineCase({
        name: "undefined-wire",
        outcomes: ["true"],
        given: { http: [{ match: { url: "https://service.example.com/x" }, reply: { status: 200 } }] },
        script: `${script}\naction.goTo("true");`,
        expect: { outcome: "true" },
      }),
      { timeoutMs: 5_000 },
    )
  ).effects;

it("an undefined top-level openidm.create field is recorded and stored as null; nested undefined still drops", async () => {
  const effects = await run('openidm.create("managed/alpha_user", "u1", { a: 1, d: undefined, n: { u: undefined } });');
  expect(effects.openidm).toEqual([{ method: "create", resource: "managed/alpha_user/u1", body: { a: 1, d: null, n: {} } }]);
  expect(effects.managedStore?.["managed/alpha_user"]?.[0]).toMatchObject({ a: 1, d: null, n: {} });
});

it("openidm.update keeps dropping undefined: unmeasured outside create", async () => {
  const effects = await run(
    'openidm.create("managed/alpha_user", "u1", { a: 1 }); openidm.update("managed/alpha_user/u1", null, { a: 2, d: undefined });',
  );
  expect(effects.openidm[1]).toEqual({ method: "update", resource: "managed/alpha_user/u1", body: { a: 2 } });
});

it.each([
  ["body: undefined", "{ method: 'POST', body: undefined }", null],
  ["nested undefined", "{ method: 'POST', body: { u: undefined, n: { u: undefined }, arr: [1, undefined] } }", { u: null, n: { u: null }, arr: [1, null] }],
  ["a string body", "{ method: 'POST', body: 'raw' }", "raw"],
])("httpClient.send records %s as sent", async (_name, options, body) => {
  const effects = await run(`httpClient.send("https://service.example.com/x", ${options}).get();`);
  expect(effects.http).toEqual([{ url: "https://service.example.com/x", method: "POST", body }]);
});

it("httpClient.send with no body key records none", async () => {
  const effects = await run('httpClient.send("https://service.example.com/x", { method: "GET" }).get();');
  expect(effects.http).toEqual([{ url: "https://service.example.com/x", method: "GET" }]);
});

it("httpClient.send records the body as it was when sent, not as mutated afterwards", async () => {
  const effects = await run('var b = { n: 1 }; httpClient.send("https://service.example.com/x", { method: "POST", body: b }).get(); b.n = 2;');
  expect(effects.http[0]?.body).toEqual({ n: 1 });
});

// docs/api/12 §5 (2026-11-04): what AIC sends for non-JSON values in an object body.
const SEND = (body: string) => `httpClient.send("https://service.example.com/x", { method: "POST", body: ${body} }).get();`;
// d0 is measured. d's unit is assumed (epoch ms): §5 sent only Date(0), where every unit gives 0.
it("httpClient.send records NaN, ±Infinity, -0 and java.util.Date as AIC sends them", async () => {
  const effects = await run(SEND("{ a: 'x', n: NaN, p: 1 / 0, m: -1 / 0, z: -0, d0: new java.util.Date(0), d: new java.util.Date(1234), arr: [NaN] }"));
  // -0 goes out as -0.0; the recorded effects are JSON, which cannot carry the sign (as 1.0 is 1).
  expect(effects.http[0]?.body).toEqual({ a: "x", n: "NaN", p: "Infinity", m: "-Infinity", z: 0, d0: 0, d: 1234, arr: ["NaN"] });
});
it("httpClient.send never calls toJSON: a toJSON method is a function in the body", async () => {
  expectHarnessFault("send-body-function");
  await expect(run(SEND("{ a: 1, toJSON: function () { return 'x'; } }"))).rejects.toThrow(/rhino-local: httpClient\.send body\.toJSON is a function: AIC sends/);
});
// §5 measured the root self-cycle (b.self = b); this nested shape is assumed to throw the same.
const CYCLE = "(function () { var b = { a: 'x', n: {} }; b.n.up = b; return b; })()";
it("a cycle throws AIC's catchable InternalError and records nothing", async () => {
  const effects = await run(`try { ${SEND(CYCLE)} } catch (e) { nodeState.putShared("err", String(e)); }`);
  expect(effects.sharedState.final.err).toBe("InternalError: Cannot produce JSON from [object Object]");
  expect(effects.http).toEqual([]);
});
it("an uncaught cycle fails the script with AIC's message", async () => {
  await expect(run(SEND(CYCLE))).rejects.toThrow(/InternalError: Cannot produce JSON from \[object Object\]/);
});
// A Date, RegExp or function goes out as a Rhino object dump nothing here reproduces, so the send
// is a harness fault: it fails the run even when the script catches it, because AIC would not throw.
const DATE = /rhino-local: httpClient\.send body\.d is a JS Date: AIC sends a ~1 KB dump of the Rhino NativeDate object without the time value, not the date \(docs\/api\/12 §5\); send date\.toISOString\(\) or date\.getTime\(\)/;
it.each([
  ["a JS Date", "send-body-js-date", "{ d: new Date(0) }", DATE],
  ["a RegExp", "send-body-regexp", "{ r: /a/g }", /rhino-local: httpClient\.send body\.r is a RegExp: AIC sends a ~1 KB dump of the Rhino NativeRegExp object without its source or flags/],
  ["a function body", "send-body-function", "function () {}", /rhino-local: httpClient\.send body is a function: AIC sends a ~1\.7 KB dump of the Rhino function object, which exposes the script's name, its encoded source and the name of every binding and variable in the script/],
  ["a nested function", "send-body-function", "{ arr: [1, function () {}] }", /rhino-local: httpClient\.send body\.arr\[1\] is a function: AIC sends/],
  ["an Error (unmeasured)", "send-body-error", "{ error: new Error('boom') }", /rhino-local: httpClient\.send body\.error is an Error: what AIC sends for one is unmeasured \(it most likely dumps the Rhino object, as it does a JS Date or RegExp, docs\/api\/12 §5\); send e\.message or String\(e\)/],
  ["a caught TypeError", "send-body-error", "(function () { try { null.x; } catch (e) { return { error: e }; } })()", /rhino-local: httpClient\.send body\.error is an Error: what AIC sends/],
  ["a cycle through an array", "send-body-array-cycle", "(function () { var b = { l: [] }; b.l.push(b); return b; })()", /rhino-local: httpClient\.send body\.l\[0\] refers back to an enclosing value through an array \(a cycle\): AIC's error for this shape is unmeasured/],
])("httpClient.send with %s fails the run by name", async (_name, id, body, message) => {
  expectHarnessFault(id);
  await expect(run(SEND(body))).rejects.toThrow(message);
});
it("a caught Date refusal still fails the run by id, and says the script may have caught it", async () => {
  expectHarnessFault("send-body-js-date");
  await expect(run(`try { ${SEND("{ d: new Date(0) }")} } catch (e) { nodeState.putShared("err", String(e)); }`)).rejects.toThrow(
    /rhino-local harness fault send-body-js-date: httpClient\.send body\.d is a JS Date[\s\S]*this fails the run even if the script caught it/,
  );
});
it.each([
  ["a shared, acyclic sub-object", "(function () { var s = { k: 1 }; return { a: s, b: s }; })()", { a: { k: 1 }, b: { k: 1 } }],
  ["a Java String", "new java.lang.String('x')", "x"],
])("httpClient.send still records %s", async (_name, body, recorded) => {
  const effects = await run(`httpClient.send("https://service.example.com/x", { method: "POST", body: ${body} }).get();`);
  expect(effects.http[0]?.body).toEqual(recorded);
});
