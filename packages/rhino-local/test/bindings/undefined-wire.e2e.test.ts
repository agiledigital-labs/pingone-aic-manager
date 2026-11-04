/**
 * docs/api/12 (2026-11-04) on the real JVM: an undefined openidm.create content field is sent as
 * null, and an httpClient.send body sends undefined as null at any depth, the body included.
 */
import { afterAll, beforeAll, expect, it } from "vitest";
import { runCase } from "../../src/bindings/index.ts";
import { defineCase } from "../../src/case/index.ts";
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

// R6-4: a body JSON cannot carry, or whose conversion AIC was never measured doing, fails by name
// at send rather than as an opaque engine error or a silent guess (NaN -> null, Date -> string).
it.each([
  ["a cycle", "(function () { var b = { n: {} }; b.n.up = b; return b; })()", /rhino-local: httpClient\.send body\.n\.up refers back to an enclosing object \(a cycle\)/],
  ["a function body", "function () {}", /rhino-local: httpClient\.send body is a function: not JSON data/],
  ["a nested function", "{ arr: [1, function () {}] }", /rhino-local: httpClient\.send body\.arr\[1\] is a function/],
  ["NaN", "{ n: NaN }", /rhino-local: httpClient\.send body\.n is NaN: not JSON data, and how AIC sends it is unmeasured/],
  ["-Infinity", "{ n: -1 / 0 }", /rhino-local: httpClient\.send body\.n is -Infinity/],
  ["a Date", "{ d: new Date(0) }", /rhino-local: httpClient\.send body\.d is a Date: how AIC serialises one is unmeasured/],
])("httpClient.send refuses %s by name", async (_name, body, message) => {
  await expect(run(`httpClient.send("https://service.example.com/x", { method: "POST", body: ${body} }).get();`)).rejects.toThrow(message);
});

it.each([
  ["a shared, acyclic sub-object", "(function () { var s = { k: 1 }; return { a: s, b: s }; })()", { a: { k: 1 }, b: { k: 1 } }],
  ["a Java String", "new java.lang.String('x')", "x"],
])("httpClient.send still records %s", async (_name, body, recorded) => {
  const effects = await run(`httpClient.send("https://service.example.com/x", { method: "POST", body: ${body} }).get();`);
  expect(effects.http[0]?.body).toEqual(recorded);
});
