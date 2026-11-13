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
import { RhinoRunner } from "../../src/runner.ts";

interface Row {
  name: string;
  script: string;
  fault: string;
  given?: Record<string, unknown>;
  /** Default: the access-token context. */
  context?: "OIDC_CLAIMS" | "SOCIAL_IDP_PROFILE_TRANSFORMATION";
}

const caught = (body: string) => `try { ${body} } catch (e) { accessToken.setField("out", "caught"); }`;
// An OIDC script that catches the fault and returns nothing AIC would refuse.
const oidcCaught = (body: string) => `var zzCaught = false; try { ${body} } catch (e) { zzCaught = true; } undefined;`;

// A social IdP transform that catches the fault and returns a profile AIC accepts.
const socialCaught = (body: string) => `var zzCaught = false; try { ${body} } catch (e) { zzCaught = true; } ({ out: "ran" });`;

const ROWS: Row[] = [
  // P2-1: non-JSON objects handed to a setter.
  { name: "P2-1 setField with a Date", script: caught(`accessToken.setField("d", new Date(0));`),
    fault: "oauth2-token-unmeasured/accessToken.setField()" },
  { name: "P2-1 setField with a RegExp inside an object",
    script: caught(`accessToken.setField("box", { re: /x/ });`), fault: "oauth2-token-unmeasured/accessToken.setField()" },
  { name: "P2-1 setField with a top-level RegExp", script: caught(`accessToken.setField("re", /x/);`),
    fault: "oauth2-token-unmeasured/accessToken.setField()" },
  { name: "P2-1 setFields with a Date", script: caught(`accessToken.setFields(new Date(0));`),
    fault: "oauth2-token-unmeasured/accessToken.setFields(object)" },
  { name: "P2-1 setField with a getter",
    script: caught(`accessToken.setField("g", { get x() { return 1; } });`), fault: "oauth2-token-unmeasured/accessToken.setField()" },
  { name: "P2-1 setAct with a Date", script: caught(`accessToken.setAct(new Date(0));`),
    fault: "oauth2-token-unmeasured/accessToken.setAct(object)" },
  // P2-3: map writes and seeded keys that collide with a method.
  { name: "P2-3 assigning a request param",
    script: caught(`requestProperties.requestParams.extra = ["changed"];`),
    fault: "oauth2-token-unmeasured/requestProperties.requestParams.extra",
    given: { request: { params: { extra: ["value"] } } } },
  { name: "P2-3 adding a request param (found by the harvest audit)",
    script: `requestProperties.requestParams.added = ["x"];`,
    fault: "oauth2-token-unmeasured/requestProperties.requestParams.added" },
  { name: "P2-3 deleting a request param (found by the harvest audit)",
    script: `delete requestProperties.requestParams.extra;`,
    fault: "oauth2-token-unmeasured/requestProperties.requestParams.extra",
    given: { request: { params: { extra: ["value"] } } } },
  { name: "P2-3 replacing a map method (found by the harvest audit)",
    script: `requestProperties.requestParams.size = function () { return 9; };`,
    fault: "oauth2-token-unmeasured/requestProperties.requestParams.size" },
  { name: "P2-3 a seeded param named like a Map method", script: `accessToken.setField("out", "ran");`,
    fault: "oauth2-token-unmeasured/requestProperties.requestParams.get",
    given: { request: { params: { get: ["value"] } } } },
  // P2-5: the tag of a view whose Java tag was not measured.
  { name: "P2-5 Object.prototype.toString of an unmeasured view",
    script: caught(`accessToken.setField("out", Object.prototype.toString.call(requestProperties));`),
    fault: "oauth2-token-unmeasured/Object.prototype.toString.call(requestProperties)" },
  // P2-6: a call count no capture measured.
  { name: "P2-6 getScope(1)", script: caught(`accessToken.getScope(1);`),
    fault: "oauth2-token-unmeasured/accessToken.getScope(1)" },
  { name: "P2-6 identity.getName(1)", script: caught(`identity.getName(1);`),
    fault: "oauth2-token-unmeasured/identity.getName(1)" },

  // Sol ctx r2.
  // P1-1: an add/remove pair through two insertion routes, then a measured-refused delete.
  { name: "r2 P1-1 claims.extra = x; remove(extra); delete claims.subname", context: "OIDC_CLAIMS",
    script: `claims.extra = "x"; claims.remove("extra"); delete claims.subname; ({ values: claims });`,
    fault: "oidc-claims-unmeasured/claims.subname" },
  // P2-1: a cyclic setter value.
  { name: "r2 P2-1 setField with a cyclic object",
    script: caught(`var value = {}; value.self = value; accessToken.setField("x", value);`),
    fault: "oauth2-token-unmeasured/accessToken.setField()" },
  { name: "r2 P2-1 setField with a cycle below the top",
    script: caught(`var inner = {}; inner.back = { up: inner }; accessToken.setField("x", { a: inner });`),
    fault: "oauth2-token-unmeasured/accessToken.setField()" },
  // P2-5: tags of identity and accessToken are unmeasured.
  { name: "r2 P2-5 Object.prototype.toString.call(identity)",
    script: caught(`accessToken.setField("out", Object.prototype.toString.call(identity));`),
    fault: "oauth2-token-unmeasured/Object.prototype.toString.call(identity)" },
  { name: "r2 P2-5 Object.prototype.toString.call(accessToken)",
    script: caught(`accessToken.setField("out", Object.prototype.toString.call(accessToken));`),
    fault: "oauth2-token-unmeasured/Object.prototype.toString.call(accessToken)" },
  { name: "r2 P2-5 Object.prototype.toString.call(identity) (OIDC)", context: "OIDC_CLAIMS",
    script: oidcCaught(`Object.prototype.toString.call(identity);`),
    fault: "oidc-claims-unmeasured/Object.prototype.toString.call(identity)" },
  // P2-6: the native surface of a real Java object.

  // Sol ctx r3.
  // P2-2: rendering a profile value nobody measured faults like reading it.
  { name: "r3 P2-2 String(rawProfile) with a boolean value", context: "SOCIAL_IDP_PROFILE_TRANSFORMATION",
    script: socialCaught(`String(rawProfile);`), fault: "social-idp-unmeasured/rawProfile.verified",
    given: { socialIdp: { rawProfile: { sub: "s", verified: true } } } },
  { name: "r3 P2-2 String(rawProfile) with a nested object", context: "SOCIAL_IDP_PROFILE_TRANSFORMATION",
    script: socialCaught(`String(rawProfile);`), fault: "social-idp-unmeasured/rawProfile.address",
    given: { socialIdp: { rawProfile: { sub: "s", address: { city: "c" } } } } },
  // P2-3: a seeded key named like a context's own measured method is refused, not hidden.
  { name: "r3 P2-3 a profile key named keys", context: "SOCIAL_IDP_PROFILE_TRANSFORMATION",
    script: socialCaught(`rawProfile.get("keys");`), fault: "social-idp-unmeasured/rawProfile.keys",
    given: { socialIdp: { rawProfile: { sub: "s", keys: "value" } } } },
];

describe("shared context views fault what no capture measured (Sol ctx r1, r2, r3)", () => {
  let runner: RhinoRunner;
  beforeAll(async () => { runner = await RhinoRunner.spawn(); }, 60_000);
  afterAll(async () => { await runner?.close(); }, 30_000);

  it.each(ROWS)("$name", async ({ name, script, fault, given, context }) => {
    expectHarnessFault(fault);
    const run = runCase(runner, context === "SOCIAL_IDP_PROFILE_TRANSFORMATION"
      ? defineCase({
        name, context, script, given: { ...given },
        expect: { allowUndeclared: { socialIdp: true, sharedState: true, transientState: true, logs: true } },
      })
      : context === "OIDC_CLAIMS"
      ? defineCase({
        name, context, script, given: { ...given },
        expect: { allowUndeclared: { oidc: true, logs: true } },
      })
      : defineCase({
        name,
        context: "OAUTH2_ACCESS_TOKEN_MODIFICATION",
        script,
        given: { ...given },
        expect: { allowUndeclared: { accessToken: true } },
      }));
    const error = await run.then(() => null, (e: unknown) => e);
    expect(error, "the run must fault").toBeInstanceOf(CaseRunError);
    const ids = ((error as CaseRunError).response.emulation?.faults ?? []).map((f) => f.id);
    expect(ids).toContain(fault);
  }, 60_000);

  // The scopes of a token script and a request value are Java lists, so Java and the engine
  // answer every call on them, at every size. Each row is something a JS imitation of a list had
  // to refuse or get wrong.
  it.each([
    ["JSON.stringify(scopes)", `return JSON.stringify(scopes);`, ["a", "b"], '["a","b"]'],
    ["[].concat(scopes) is a JS array of its elements", `var c = [].concat(scopes); return Array.isArray(c) + "|" + c.length + "|" + (c[1] === "b");`, ["a", "b"], "true|2|true"],
    ["a borrowed Array.prototype.map", `return Array.prototype.map.call(scopes, function (x) { return x + "!"; }).join();`, ["a", "b"], "a!,b!"],
    ["no Array.prototype member of its own", `return typeof scopes.map + "|" + typeof scopes.join + "|" + Array.isArray(scopes);`, ["a"], "undefined|undefined|false"],
    ["an index write is a write to the list", `scopes[0] = "changed"; scopes.add("b"); return [scopes[0], scopes.get(0), scopes.size(), String(scopes)].join("|");`, ["a"], "changed|changed|2|[changed, b]"],
    ["get() past the end is Java's refusal", `try { scopes.get(1); } catch (x) { return x.name + ": " + String(x.message).split(" (")[0]; }`, ["a"], "InternalError: Index 1 out of bounds for length 1"],
    ["a read past the end by index is undefined", `return String(scopes[1]);`, ["a"], "undefined"],
    ["the tag", `return Object.prototype.toString.call(scopes);`, ["a"], "[object JavaList]"],
    ["empty: JSON, String, length, size, isEmpty", `return [JSON.stringify(scopes), String(scopes), scopes.length, scopes.size(), scopes.isEmpty()].join("|");`, [], "[]|[]|0|0|true"],
    ["empty: concat, a borrowed map, contains, indexOf", `return [[].concat(scopes).length, Array.prototype.map.call(scopes, String).length, scopes.contains("a"), scopes.indexOf("a")].join("|");`, [], "0|0|false|-1"],
    ["empty: forEach calls nothing, and get(0) is Java's refusal", `var n = 0; scopes.forEach(function () { n += 1; }); try { scopes.get(0); } catch (x) { return n + "|" + x.name + ": " + String(x.message).split(" (")[0]; }`, [], "0|InternalError: Index 0 out of bounds for length 0"],
  ])("a scopes list answers as a Java list: %s", async (name, read, scopes, want) => {
    const { effects, verdict } = await runCase(runner, defineCase({
      name,
      context: "OAUTH2_ACCESS_TOKEN_MODIFICATION",
      script: `accessToken.setField("out", String((function () { ${read} })()));`,
      given: { engine: "next-gen", scopes },
      expect: { accessToken: { fields: { added: { out: want } } } },
    }));
    expect(verdict.pass, verdict.summary).toBe(true);
    expect(effects.accessToken!.final.fields.out).toBe(want);
  }, 60_000);

  it("a request value list is a Java list too, empty included", async () => {
    const want = '["value"]|1|[value]|true|[]|0|[]';
    const { effects, verdict } = await runCase(runner, defineCase({
      name: "request value list",
      context: "OAUTH2_ACCESS_TOKEN_MODIFICATION",
      script: `var p = requestProperties.requestParams, v = p.extra, e = p.none; accessToken.setField("out", [JSON.stringify(v), v.length, String(v), v.contains("value"), JSON.stringify(e), e.size(), String(e)].join("|"));`,
      given: { engine: "next-gen", request: { params: { extra: ["value"], none: [] } } },
      expect: { accessToken: { fields: { added: { out: want } } } },
    }));
    expect(verdict.pass, verdict.summary).toBe(true);
    expect(effects.accessToken!.final.fields.out).toBe(want);
  }, 60_000);

  it("keeps the measured refusals ordinary: setField with one argument is Rhino's overload error", async () => {
    const { effects } = await runCase(runner, defineCase({
      name: "setField arity",
      context: "OAUTH2_ACCESS_TOKEN_MODIFICATION",
      script: `try { accessToken.setField("only"); } catch (e) { accessToken.setField("out", String(e).indexOf("Can't find method") !== -1); }`,
      expect: { allowUndeclared: { accessToken: true } },
    }));
    expect(effects.accessToken!.final.fields.out).toBe(true);
  }, 60_000);

  it("r2 controls: a repeated reference is not a cycle, the measured scopes.add() moves the baseline", async () => {
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
      // Two measured adds in a row: the r3 pre-add audit finds nothing to fault.
      script: `scopes.add("zz-added"); scopes.add("zz-again"); accessToken.setField("out", scopes.size());`,
      given: { engine: "next-gen", scopes: ["a"] },
      expect: { allowUndeclared: { accessToken: true } },
    }));
    expect(added.effects.accessToken!.final.fields.out).toBe(3);
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
