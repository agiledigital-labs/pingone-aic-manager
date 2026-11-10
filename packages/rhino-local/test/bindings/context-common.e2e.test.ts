/**
 * The shared context view helpers (src/bindings/rhino/context-common.cjs), on
 * the real JVM, through the access-token context that first used them. Each
 * row is a script a reviewer showed passing a strict case while doing
 * something no capture measured (Sol ctx r1). Every one must now end the run
 * with the named, tallied fault, and catching the fault must not rescue it.
 */
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { CaseRunError, runCase } from "../../src/bindings/run.ts";
import { expectHarnessFault } from "../../src/harness/emulation-guard.ts";
import { defineCase } from "../../src/case/validate.ts";
import type { Engine } from "../../src/case/types.ts";
import { RhinoRunner } from "../../src/runner.ts";

interface Row {
  name: string;
  engines: readonly Engine[];
  script: string;
  fault: string;
  given?: Record<string, unknown>;
}

const both = ["next-gen", "legacy"] as const;
const caught = (body: string) => `try { ${body} } catch (e) { accessToken.setField("out", "caught"); }`;

const ROWS: Row[] = [
  // P2-1: non-JSON objects handed to a setter.
  { name: "P2-1 setField with a Date", engines: both, script: caught(`accessToken.setField("d", new Date(0));`),
    fault: "oauth2-token-unmeasured/accessToken.setField()" },
  { name: "P2-1 setField with a RegExp inside an object", engines: both,
    script: caught(`accessToken.setField("box", { re: /x/ });`), fault: "oauth2-token-unmeasured/accessToken.setField()" },
  { name: "P2-1 setField with a top-level RegExp", engines: both, script: caught(`accessToken.setField("re", /x/);`),
    fault: "oauth2-token-unmeasured/accessToken.setField()" },
  { name: "P2-1 setFields with a Date", engines: both, script: caught(`accessToken.setFields(new Date(0));`),
    fault: "oauth2-token-unmeasured/accessToken.setFields(object)" },
  { name: "P2-1 setField with a getter", engines: both,
    script: caught(`accessToken.setField("g", { get x() { return 1; } });`), fault: "oauth2-token-unmeasured/accessToken.setField()" },
  { name: "P2-1 setAct with a Date", engines: ["next-gen"], script: caught(`accessToken.setAct(new Date(0));`),
    fault: "oauth2-token-unmeasured/accessToken.setAct(object)" },
  // P2-2: list serialisation and borrowed builtins.
  { name: "P2-2 JSON.stringify(scopes)", engines: ["next-gen"], script: caught(`accessToken.setField("out", JSON.stringify(scopes));`),
    fault: "oauth2-token-unmeasured/list" },
  { name: "P2-2 [].concat(scopes)", engines: ["next-gen"], script: caught(`accessToken.setField("out", [].concat(scopes).length);`),
    fault: "oauth2-token-unmeasured/list" },
  { name: "P2-2 Array.prototype.map.call(scopes)", engines: ["next-gen"],
    script: caught(`Array.prototype.map.call(scopes, function (x) { return x + "!"; });`), fault: "oauth2-token-unmeasured/list" },
  { name: "P2-2 JSON.stringify of a legacy empty audience list", engines: ["legacy"],
    script: caught(`accessToken.setField("out", JSON.stringify(accessToken.getAudience()));`), fault: "oauth2-token-unmeasured/list",
    given: { accessToken: { audience: [] } } },
  // P2-3: map writes and seeded keys that collide with a method.
  { name: "P2-3 assigning a request param", engines: both,
    script: caught(`requestProperties.requestParams.extra = ["changed"];`),
    fault: "oauth2-token-unmeasured/requestProperties.requestParams.extra",
    given: { request: { params: { extra: ["value"] } } } },
  { name: "P2-3 adding a request param (found by the harvest audit)", engines: both,
    script: `requestProperties.requestParams.added = ["x"];`,
    fault: "oauth2-token-unmeasured/requestProperties.requestParams.added" },
  { name: "P2-3 deleting a request param (found by the harvest audit)", engines: both,
    script: `delete requestProperties.requestParams.extra;`,
    fault: "oauth2-token-unmeasured/requestProperties.requestParams.extra",
    given: { request: { params: { extra: ["value"] } } } },
  { name: "P2-3 replacing a map method (found by the harvest audit)", engines: both,
    script: `requestProperties.requestParams.size = function () { return 9; };`,
    fault: "oauth2-token-unmeasured/requestProperties.requestParams.size" },
  { name: "P2-3 a seeded param named like a Map method", engines: both, script: `accessToken.setField("out", "ran");`,
    fault: "oauth2-token-unmeasured/requestProperties.requestParams.get",
    given: { request: { params: { get: ["value"] } } } },
  // P2-5: the tag of a view whose Java tag was not measured.
  { name: "P2-5 Object.prototype.toString of an unmeasured view", engines: both,
    script: caught(`accessToken.setField("out", Object.prototype.toString.call(requestProperties));`),
    fault: "oauth2-token-unmeasured/Object.prototype.toString.call(requestProperties)" },
  // P2-6: a call count no capture measured.
  { name: "P2-6 getScope(1)", engines: ["legacy", "next-gen"], script: caught(`accessToken.getScope(1);`),
    fault: "oauth2-token-unmeasured/accessToken.getScope(1)" },
  { name: "P2-6 identity.getName(1)", engines: both, script: caught(`identity.getName(1);`),
    fault: "oauth2-token-unmeasured/identity.getName(1)" },
];

describe("shared context views fault what no capture measured (Sol ctx r1)", () => {
  let runner: RhinoRunner;
  beforeAll(async () => { runner = await RhinoRunner.spawn(); }, 60_000);
  afterAll(async () => { await runner?.close(); }, 30_000);

  const cases = ROWS.flatMap((row) => row.engines.map((engine) => ({ ...row, engine })));
  it.each(cases)("$name ($engine)", async ({ name, engine, script, fault, given }) => {
    expectHarnessFault(fault);
    const run = runCase(runner, defineCase({
      name: `${name} ${engine}`,
      context: "OAUTH2_ACCESS_TOKEN_MODIFICATION",
      script,
      given: { engine, ...given },
      expect: { allowUndeclared: { accessToken: true } },
    }));
    const error = await run.then(() => null, (e: unknown) => e);
    expect(error, "the run must fault").toBeInstanceOf(CaseRunError);
    const ids = ((error as CaseRunError).response.emulation?.faults ?? []).map((f) => f.id);
    expect(ids).toContain(fault);
  }, 60_000);

  it("keeps the measured refusals ordinary: setField with one argument is Rhino's overload error on both engines", async () => {
    for (const engine of both) {
      const { effects } = await runCase(runner, defineCase({
        name: `setField arity ${engine}`,
        context: "OAUTH2_ACCESS_TOKEN_MODIFICATION",
        script: `try { accessToken.setField("only"); } catch (e) { accessToken.setField("out", String(e).indexOf("Can't find method") !== -1); }`,
        given: { engine },
        expect: { allowUndeclared: { accessToken: true } },
      }));
      expect(effects.accessToken!.final.fields.out, engine).toBe(true);
    }
  }, 60_000);

  it("emulates the measured legacy JsonValue tag, [object JavaObject]", async () => {
    const { effects } = await runCase(runner, defineCase({
      name: "JsonValue tag",
      context: "OAUTH2_ACCESS_TOKEN_MODIFICATION",
      script: `accessToken.setField("s", "x"); accessToken.setField("out", Object.prototype.toString.call(accessToken.getField("s")));`,
      given: { engine: "legacy" },
      expect: { allowUndeclared: { accessToken: true } },
    }));
    expect(effects.accessToken!.final.fields.out).toBe("[object JavaObject]");
  }, 60_000);

  it("still accepts plain JSON values, including an Object.create(null) object and a __proto__ key", async () => {
    const { effects } = await runCase(runner, defineCase({
      name: "plain JSON",
      context: "OAUTH2_ACCESS_TOKEN_MODIFICATION",
      script: `var bare = Object.create(null); bare.k = [1, "two", { deep: true }];
accessToken.setField("bare", bare);
accessToken.setField("__proto__", { x: 1 });`,
      given: { engine: "next-gen" },
      expect: { allowUndeclared: { accessToken: true } },
    }));
    const fields = effects.accessToken!.final.fields;
    expect(fields.bare).toEqual({ k: [1, "two", { deep: true }] });
    expect(Object.hasOwn(fields, "__proto__")).toBe(true);
  }, 60_000);
});
