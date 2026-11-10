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
  /** Default: the access-token context. */
  context?: "OIDC_CLAIMS";
}

const both = ["next-gen", "legacy"] as const;
const caught = (body: string) => `try { ${body} } catch (e) { accessToken.setField("out", "caught"); }`;
// An OIDC script that catches the fault and returns nothing AIC would refuse.
const oidcCaught = (body: string) => `var zzCaught = false; try { ${body} } catch (e) { zzCaught = true; } undefined;`;

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

  // Sol ctx r2.
  // P1-1: an add/remove pair through two insertion routes, then a measured-refused delete.
  { name: "r2 P1-1 claims.extra = x; remove(extra); delete claims.subname", engines: ["next-gen"], context: "OIDC_CLAIMS",
    script: `claims.extra = "x"; claims.remove("extra"); delete claims.subname; ({ values: claims });`,
    fault: "oidc-claims-unmeasured/claims.subname" },
  // P2-1: a cyclic setter value.
  { name: "r2 P2-1 setField with a cyclic object", engines: both,
    script: caught(`var value = {}; value.self = value; accessToken.setField("x", value);`),
    fault: "oauth2-token-unmeasured/accessToken.setField()" },
  { name: "r2 P2-1 setField with a cycle below the top", engines: both,
    script: caught(`var inner = {}; inner.back = { up: inner }; accessToken.setField("x", { a: inner });`),
    fault: "oauth2-token-unmeasured/accessToken.setField()" },
  // P2-3: writes to a list view, found by the harvest audit.
  { name: "r2 P2-3 scopes[0] = changed (OIDC)", engines: ["next-gen"], context: "OIDC_CLAIMS",
    script: `scopes[0] = "changed"; ({ values: { out: scopes[0] } });`, fault: "oidc-claims-unmeasured/scopes.0",
    given: { scopes: ["a"] } },
  { name: "r2 P2-3 replacing scopes.size (OIDC)", engines: ["next-gen"], context: "OIDC_CLAIMS",
    script: `scopes.size = function () { return 99; }; ({ values: { size: scopes.size() } });`,
    fault: "oidc-claims-unmeasured/scopes.size", given: { scopes: ["a"] } },
  { name: "r2 P2-3 scopes[0] = changed (token)", engines: ["next-gen"],
    script: `scopes[0] = "changed"; accessToken.setField("out", "ran");`, fault: "oauth2-token-unmeasured/scopes.0" },
  // Legacy scopes is a Java Set view, which refuses the write itself.
  { name: "r2 P2-3 scopes[0] = changed (token)", engines: ["legacy"],
    script: `scopes[0] = "changed"; accessToken.setField("out", "ran");`, fault: "oauth2-token-unmeasured/scopes[0]" },
  { name: "r2 P2-3 writing into a request value list", engines: both,
    script: `requestProperties.requestParams.extra[0] = "changed"; accessToken.setField("out", "ran");`,
    fault: "oauth2-token-unmeasured/list.0", given: { request: { params: { extra: ["value"] } } } },
  // P2-5: tags of identity and accessToken are unmeasured.
  { name: "r2 P2-5 Object.prototype.toString.call(identity)", engines: both,
    script: caught(`accessToken.setField("out", Object.prototype.toString.call(identity));`),
    fault: "oauth2-token-unmeasured/Object.prototype.toString.call(identity)" },
  { name: "r2 P2-5 Object.prototype.toString.call(accessToken)", engines: both,
    script: caught(`accessToken.setField("out", Object.prototype.toString.call(accessToken));`),
    fault: "oauth2-token-unmeasured/Object.prototype.toString.call(accessToken)" },
  { name: "r2 P2-5 Object.prototype.toString.call(identity) (OIDC)", engines: both, context: "OIDC_CLAIMS",
    script: oidcCaught(`Object.prototype.toString.call(identity);`),
    fault: "oidc-claims-unmeasured/Object.prototype.toString.call(identity)" },
  // P2-6: the native surface of a real Java object.
  { name: "r2 P2-6 identity.getName().substring (OIDC)", engines: ["legacy"], context: "OIDC_CLAIMS",
    script: oidcCaught(`identity.getName().substring(0, 1);`),
    fault: "oidc-claims-unmeasured/java.lang.String.substring" },
  { name: "r2 P2-6 identity.getName().substring (token)", engines: ["legacy"],
    script: caught(`accessToken.setField("out", String(identity.getName().substring(0, 1)));`),
    fault: "oauth2-token-unmeasured/java.lang.String.substring" },
  { name: "r2 P2-6 a JS String.prototype method on a Java string", engines: ["legacy"],
    script: caught(`accessToken.setField("out", identity.getName().slice(0, 1));`),
    fault: "oauth2-token-unmeasured/java.lang.String.slice" },
  { name: "r2 P2-6 claims.get(sub).substring", engines: ["legacy"], context: "OIDC_CLAIMS",
    script: oidcCaught(`claims.get("sub").substring(0, 1);`),
    fault: "oidc-claims-unmeasured/java.lang.String.substring" },
  { name: "r2 P2-6 claims.clear()", engines: ["legacy"], context: "OIDC_CLAIMS",
    script: oidcCaught(`claims.clear();`), fault: "oidc-claims-unmeasured/claims.clear" },
  { name: "r2 P2-6 claims.size(1)", engines: ["legacy"], context: "OIDC_CLAIMS",
    script: oidcCaught(`claims.size(1);`), fault: "oidc-claims-unmeasured/claims.size(1 argument(s))" },
  { name: "r2 P2-6 claims.get() with no argument", engines: ["legacy"], context: "OIDC_CLAIMS",
    script: oidcCaught(`claims.get();`), fault: "oidc-claims-unmeasured/claims.get(0 argument(s))" },
  { name: "r2 P2-6 assigning a legacy claims member", engines: ["legacy"], context: "OIDC_CLAIMS",
    script: oidcCaught(`claims.zz = "a";`), fault: "oidc-claims-unmeasured/claims.zz" },
];

describe("shared context views fault what no capture measured (Sol ctx r1, r2)", () => {
  let runner: RhinoRunner;
  beforeAll(async () => { runner = await RhinoRunner.spawn(); }, 60_000);
  afterAll(async () => { await runner?.close(); }, 30_000);

  const cases = ROWS.flatMap((row) => row.engines.map((engine) => ({ ...row, engine })));
  it.each(cases)("$name ($engine)", async ({ name, engine, script, fault, given, context }) => {
    expectHarnessFault(fault);
    const run = runCase(runner, context === "OIDC_CLAIMS"
      ? defineCase({
        name: `${name} ${engine}`, context, script, given: { engine, ...given },
        expect: { allowUndeclared: { oidc: true, logs: true } },
      })
      : defineCase({
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

  it("r2 controls: a repeated reference is not a cycle, the measured scopes.add() moves the baseline, and legacy Java strings keep their measured surface", async () => {
    const shared = await runCase(runner, defineCase({
      name: "repeated reference",
      context: "OAUTH2_ACCESS_TOKEN_MODIFICATION",
      script: `var leaf = { k: 1 }; accessToken.setField("x", { a: leaf, b: [leaf, leaf] });`,
      given: { engine: "next-gen" },
      expect: { allowUndeclared: { accessToken: true } },
    }));
    expect(shared.effects.accessToken!.final.fields.x).toEqual({ a: { k: 1 }, b: [{ k: 1 }, { k: 1 }] });
    const added = await runCase(runner, defineCase({
      name: "scopes.add",
      context: "OAUTH2_ACCESS_TOKEN_MODIFICATION",
      script: `scopes.add("zz-added"); accessToken.setField("out", scopes.size());`,
      given: { engine: "next-gen", scopes: ["a"] },
      expect: { allowUndeclared: { accessToken: true } },
    }));
    expect(added.effects.accessToken!.final.fields.out).toBe(2);
    const strings = await runCase(runner, defineCase({
      name: "legacy Java string surface",
      context: "OAUTH2_ACCESS_TOKEN_MODIFICATION",
      script: `(function () {
  var n = identity.getName();
  accessToken.setField("out", [typeof n, n == "u", n === "u", typeof n.length, String(n), n + "!", n.toString() == "u",
    Object.prototype.toString.call(n), n == identity.getName(), n === identity.getName()].join(","));
})();`,
      given: { engine: "legacy", identity: { name: "u" } },
      expect: { allowUndeclared: { accessToken: true } },
    }));
    expect(strings.effects.accessToken!.final.fields.out).toBe("object,true,false,function,u,u!,true,[object JavaObject],true,true");
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
