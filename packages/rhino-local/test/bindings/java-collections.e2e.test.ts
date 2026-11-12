/**
 * The lists a scripted decision is handed are Java lists, and the service
 * objects beside them refuse what they do not have. Every case runs a script
 * through the case API on the real JVM.
 */
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { CaseRunError, runCase } from "../../src/bindings/run.ts";
import { defineCase } from "../../src/case/validate.ts";
import type { Given } from "../../src/case/types.ts";
import { expectHarnessFault } from "../../src/harness/emulation-guard.ts";
import { RhinoRunner } from "../../src/runner.ts";

// Each script puts its answer in shared state and takes the one outcome.
const node = (body: string) => `nodeState.putShared("out", String((function () { ${body} })())); action.goTo("true");`;

const given = {
  requestParameters: { many: ["alpha", "bravo"], none: [] },
  requestHeaders: { "X-Many": ["alpha", "bravo"], "X-None": [] },
  managed: {
    "managed/alpha_user": [
      { _id: "u1", _rev: "1", userName: "zzemu-one" },
      { _id: "u2", _rev: "1", userName: "zzemu-two" },
    ],
    "managed/alpha_thing": [{ _id: "t1", _rev: "1", count: 3, ratio: 1.5, on: true, gone: null }],
  },
} as Given;

const QUERY = 'openidm.query("managed/alpha_user", { _queryFilter: "true" }, ["userName"]).result';
const NOTHING = 'openidm.query("managed/alpha_user", { _queryFilter: "false" }, ["userName"]).result';
const refusal = (call: string) => `try { ${call}; return "returned"; } catch (x) { return x.name + ": " + String(x.message).split(" (")[0]; }`;

describe("a scripted decision's lists are Java lists", () => {
  let runner: RhinoRunner;
  beforeAll(async () => { runner = await RhinoRunner.spawn(); }, 60_000);
  afterAll(async () => { await runner?.close(); }, 30_000);

  const answer = async (name: string, body: string) => {
    const { effects, verdict } = await runCase(runner, defineCase({
      name,
      script: node(body),
      given,
      expect: { outcome: "true", allowUndeclared: { sharedState: true, openidmReads: true } },
    }));
    expect(verdict.pass, verdict.summary).toBe(true);
    return effects.sharedState.final.out;
  };

  // The left column of each pair is what an array standing in for the list would have said.
  it.each([
    // Measured (docs/api/12): the tag, typeof an element, and no Array.prototype member.
    ["the tag and the absent array methods", `var v = requestParameters.get("many"); return [Object.prototype.toString.call(v), Array.isArray(v), typeof v.map, typeof v[0]].join("|");`,
      "[object JavaList]|false|undefined|string"],
    // Measured: String(v) is Java's form, JSON a JSON array, [].concat(v) a JS array.
    ["String, JSON and concat", `var v = requestHeaders.get("x-many"); return [String(v), JSON.stringify(v), Array.isArray([].concat(v)), [].concat(v).join("+")].join("|");`,
      '[alpha, bravo]|["alpha","bravo"]|true|alpha+bravo'],
    ["the List members answer", `var v = requestParameters.get("many"); return [v.size(), v.get(1), v.contains("bravo"), v.indexOf("bravo"), v.isEmpty(), v.toArray().length, Array.isArray(v.toArray())].join("|");`,
      "2|bravo|true|1|false|2|false"],
    ["forEach hands each element as a string", `var seen = []; requestParameters.get("many").forEach(function (x) { seen.push(typeof x + ":" + x); }); return seen.join();`,
      "string:alpha,string:bravo"],
    // Measured: the iterator's class is off the allow-list, so asking for one is refused.
    ["iterator() is refused by class", refusal('requestParameters.get("many").iterator()'),
      'InternalError: Access to Java class "java.util.ArrayList$Itr" is prohibited.'],
    ["get() past the end is Java's refusal", refusal('requestParameters.get("many").get(2)'),
      "InternalError: Index 2 out of bounds for length 2"],
    ["get() of a non-number is the engine's conversion refusal", refusal('requestParameters.get("many").get("x")'),
      "InternalError: Cannot convert x to java.lang.Integer"],
    ["a missing method names the list in Java's form", refusal('requestParameters.get("many").some(function () {})'),
      "TypeError: Cannot find function some in object [alpha, bravo]."],
    ["an empty parameter list", `var v = requestParameters.get("none"); return [v.length, v.size(), String(v), JSON.stringify(v), [].concat(v).length, v.isEmpty(), v.contains("a"), typeof v[0]].join("|");`,
      "0|0|[]|[]|0|true|false|undefined"],
    ["an empty header list", `var v = requestHeaders.get("x-none"); var n = 0; v.forEach(function () { n += 1; }); return [v.length, String(v), JSON.stringify({ v: v }), n].join("|");`,
      '0|[]|{"v":[]}|0'],
    ["get(0) of an empty list", refusal('requestParameters.get("none").get(0)'),
      "InternalError: Index 0 out of bounds for length 0"],
    // Measured: a query result is a list of records, with length, size() and [i].
    ["a query result", `var r = ${QUERY}; return [Object.prototype.toString.call(r), Array.isArray(r), r.length, r.size(), r[1].userName, r.get(0).userName, typeof r.map, JSON.stringify(r)].join("|");`,
      '[object JavaList]|false|2|2|zzemu-two|zzemu-one|undefined|[{"_id":"u1","_rev":"1","userName":"zzemu-one"},{"_id":"u2","_rev":"1","userName":"zzemu-two"}]'],
    ["a query result's other List members", `var r = ${QUERY}; var names = []; r.forEach(function (x) { names.push(x.userName); }); return [names.join(), r.isEmpty(), r.toArray().length, r.contains(r[0]), r.indexOf(r[1])].join("|");`,
      "zzemu-one,zzemu-two|false|2|true|1"],
    ["an empty query result", `var r = ${NOTHING}; var n = 0; r.forEach(function () { n += 1; }); return [r.length, r.size(), String(r), JSON.stringify(r), [].concat(r).length, r.isEmpty(), n].join("|");`,
      "0|0|[]|[]|0|true|0"],
    // The number's form is the script engine's; AIC's was not measured.
    ["String of a query result holding numbers", `return String(openidm.query("managed/alpha_thing", { _queryFilter: "true" }).result);`,
      "[{_id=t1, _rev=1, count=3, ratio=1.5, on=true, gone=null}]"],
    // A number a script adds is held as Java holds it, and prints that way.
    ["String of a list a script added numbers to", `var v = requestParameters.get("many"); v.add(5); v.add(1.5); return String(v);`,
      "[alpha, bravo, 5.0, 1.5]"],
    // Measured: get(0) on an empty result.
    ["get(0) of an empty query result", refusal(`${NOTHING}.get(0)`),
      "InternalError: Index 0 out of bounds for length 0"],
  ])("%s", async (name, body, want) => {
    expect(await answer(name, body)).toBe(want);
  }, 60_000);
});

describe("node state keys are a Java set and a map's values a Java list", () => {
  let runner: RhinoRunner;
  beforeAll(async () => { runner = await RhinoRunner.spawn(); }, 60_000);
  afterAll(async () => { await runner?.close(); }, 30_000);

  const answer = async (name: string, body: string, attributes: Record<string, string>) => {
    const { effects, verdict } = await runCase(runner, defineCase({
      name,
      script: node(body),
      given: { registeredObjectAttributes: attributes },
      expect: { outcome: "true", allowUndeclared: { sharedState: true } },
    }));
    expect(verdict.pass, verdict.summary).toBe(true);
    return effects.sharedState.final.out;
  };
  const MAP = 'nodeState.get("objectAttributes")';

  it.each([
    // Measured (docs/api/12): keys() is a Java set, iterated with iterator(), and it lists a
    // key put earlier in the script. An array standing in for it had map, length and get.
    ["keys() is not an array", `nodeState.putShared("zz", 1); var k = nodeState.keys(); return [Array.isArray(k), typeof k.map, typeof k.length, typeof k.get, k.contains("zz"), k.contains("nope"), k.isEmpty()].join("|");`,
      "false|undefined|undefined|undefined|true|false|false"],
    ["keys() is walked with iterator()", `nodeState.putShared("zz", 1); var it = nodeState.keys().iterator(); var seen = []; while (it.hasNext()) { seen.push(String(it.next())); } return seen.indexOf("zz") !== -1 && seen.length === nodeState.keys().size();`,
      "true"],
    ["an array method on keys() is the engine's refusal", `try { nodeState.keys().map(function (x) { return x; }); return "returned"; } catch (e) { return e.name + ": " + String(e.message).split(" in object ")[0]; }`,
      "TypeError: Cannot find function map"],
    // Measured: values() is a snapshot list; size(), get(i) and toArray() work and iterator() is refused.
    ["values() is a Java list", `var v = ${MAP}.values(); return [Object.prototype.toString.call(v), Array.isArray(v), typeof v.map, v.size(), v.get(0), v.toArray().length, v.contains("p")].join("|");`,
      "[object JavaList]|false|undefined|2|p|2|true"],
    ["values() is a snapshot", `var m = ${MAP}; var v = m.values(); m.put("later", "l"); return v.size() + "|" + m.values().size();`,
      "2|3"],
    ["values().iterator() is refused by class", refusal(`${MAP}.values().iterator()`),
      'InternalError: Access to Java class "java.util.ArrayList$Itr" is prohibited.'],
    ["an array method on values() is the engine's refusal", refusal(`${MAP}.values().join()`),
      "TypeError: Cannot find function join in object [p, q]."],
  ])("%s", async (name, body, want) => {
    expect(await answer(name, body, { plain: "p", other: "q" })).toBe(want);
  }, 60_000);

  it("an empty map's values() is an empty list", async () => {
    expect(await answer("empty values", `var v = ${MAP}.values(); return [v.size(), v.isEmpty(), String(v), v.toArray().length, JSON.stringify(v)].join("|");`, {}))
      .toBe("0|true|[]|0|[]");
  }, 60_000);

  // On a tenant a decision node always holds the engine's own keys, so an empty key set can't
  // be reached by seeding; the test empties the set itself.
  it("keys() minus everything is an empty set", async () => {
    expect(await answer("empty keys", `var k = nodeState.keys(); k.clear(); return [k.size(), k.isEmpty(), String(k), k.iterator().hasNext()].join("|");`, {}))
      .toBe("0|true|[]|false");
  }, 60_000);
});

describe("a service object refuses a member the harness does not have", () => {
  let runner: RhinoRunner;
  beforeAll(async () => { runner = await RhinoRunner.spawn(); }, 60_000);
  afterAll(async () => { await runner?.close(); }, 30_000);

  it.each([
    ["openidm", "openidm.frobnicate()", "not-implemented/openidm.frobnicate"],
    ["httpClient", "httpClient.frobnicate(1, 2)", "not-implemented/httpClient.frobnicate"],
    ["nodeState", "nodeState.frobnicate()", "not-implemented/nodeState.frobnicate"],
    ["utils", "utils.frobnicate()", "not-implemented/utils.frobnicate"],
    ["an object nested in utils", "utils.crypto.subtle.frobnicate()", "not-implemented/utils.crypto.subtle.frobnicate"],
    ["idRepository", "idRepository.frobnicate()", "not-implemented/idRepository.frobnicate"],
    ["callbacksBuilder", "callbacksBuilder.frobnicate()", "not-implemented/callbacksBuilder.frobnicate"],
    ["logger", "logger.frobnicate()", "not-implemented/logger.frobnicate"],
    ["secrets", "secrets.frobnicate()", "not-implemented/secrets.frobnicate"],
    ["systemEnv", "systemEnv.frobnicate()", "not-implemented/systemEnv.frobnicate"],
    ["requestParameters", "requestParameters.frobnicate()", "not-implemented/requestParameters.frobnicate"],
    ["requestHeaders", "requestHeaders.frobnicate()", "not-implemented/requestHeaders.frobnicate"],
    ["requestCookies", "requestCookies.frobnicate()", "not-implemented/requestCookies.frobnicate"],
    // An object a service hands back is named by the call that made it.
    ["an HTTP response", 'httpClient.send("https://service.example.com/x").get().frobnicate()', "not-implemented/httpClient.send().get().frobnicate"],
    ["a pending HTTP response", 'httpClient.send("https://service.example.com/x").frobnicate()', "not-implemented/httpClient.send().frobnicate"],
    ["a state object read back", 'nodeState.get("objectAttributes").frobnicate()', "not-implemented/nodeState.get().frobnicate"],
    // goTo returns the action itself.
    ["the action a goTo returns", 'action.goTo("true").frobnicate()', "not-implemented/action.frobnicate"],
  ])("%s: an unknown method is a named fault that fails the run though the script caught it", async (name, call, id) => {
    expectHarnessFault(id);
    const error = await runCase(runner, defineCase({
      name: `unknown member of ${name}`,
      script: `try { ${call}; } catch (e) {} action.goTo("true");`,
      given: { ...given, registeredObjectAttributes: { mail: "zzemu@example.com" }, http: [{ match: { url: "https://service.example.com/x" }, reply: { status: 200 } }] },
      expect: { outcome: "true", allowUndeclared: { sharedState: true, http: true, openidmReads: true } },
    })).then(() => null, (e: unknown) => e);
    expect(error).toBeInstanceOf(CaseRunError);
    const faults = (error as CaseRunError).response.emulation?.faults ?? [];
    expect(faults.map((f) => f.id)).toEqual([id]);
    expect((error as CaseRunError).message).toContain(`rhino-local harness fault ${id}`);
  }, 60_000);

  it("a method AIC declares and the harness has not written is a named fault too", async () => {
    const id = "not-mocked/jwtValidator.validateJwtClaims";
    expectHarnessFault(id);
    const error = await runCase(runner, defineCase({
      name: "declared, not written",
      script: `try { jwtValidator.validateJwtClaims({}); } catch (e) {} action.goTo("true");`,
      expect: { outcome: "true" },
    })).then(() => null, (e: unknown) => e);
    expect(error).toBeInstanceOf(CaseRunError);
    expect(((error as CaseRunError).response.emulation?.faults ?? []).map((f) => f.id)).toEqual([id]);
  }, 60_000);

  // Measured (docs/api/12): the next-gen logger has no message() or warningEnabled().
  it("a method measured absent on AIC stays the engine's TypeError, and the run passes", async () => {
    const { effects, verdict } = await runCase(runner, defineCase({
      name: "measured absent",
      script: node(`return [${JSON.stringify(refusal("logger.message('x')"))}, ${JSON.stringify(refusal("logger.warningEnabled()"))}].map(function (f) { return new Function(f)().split(" in object ")[0]; }).join("|");`),
      expect: { outcome: "true", allowUndeclared: { sharedState: true } },
    }));
    expect(verdict.pass, verdict.summary).toBe(true);
    expect(effects.sharedState.final.out).toBe("TypeError: Cannot find function message|TypeError: Cannot find function warningEnabled");
  }, 60_000);

  // What AIC answers with a TypeError stays one: a record is the script's own
  // data, an identity has no getAttribute (measured), and a map entry named
  // like a method hides the method (measured).
  it("plain data and measured absences on a returned object are still the engine's TypeError", async () => {
    const { effects, verdict } = await runCase(runner, defineCase({
      name: "returned data",
      script: node(`return [` +
        `(function () { ${refusal('openidm.read("managed/alpha_user/u1").frobnicate()')} })(), ` +
        `(function () { ${refusal('idRepository.getIdentity("u1").getAttribute("mail")')} })(), ` +
        `(function () { ${refusal('nodeState.get("objectAttributes").get("x")')} })()].map(function (m) { return m.split(" in object ")[0]; }).join("|");`),
      given: { ...given, registeredObjectAttributes: { get: "entry" } },
      expect: { outcome: "true", allowUndeclared: { sharedState: true, openidmReads: true } },
    }));
    expect(verdict.pass, verdict.summary).toBe(true);
    expect(effects.sharedState.final.out).toBe(
      "TypeError: Cannot find function frobnicate|TypeError: Cannot find function getAttribute|TypeError: Cannot call property get");
  }, 60_000);

  // The identity's member set was captured whole. A name outside it is absent on AIC; a name
  // inside it that the harness has not written is the fault.
  it.each([
    ["size", false], ["isExists", false], ["frobnicateToo", false],
    ["hashCode", true], ["getClass", true], ["equals", true],
  ])("an identity's %s() follows the captured member set", async (member, has) => {
    const id = `not-implemented/idRepository.getIdentity().${member}`;
    if (has) expectHarnessFault(id);
    const run = runCase(runner, defineCase({
      name: `identity ${member}`,
      script: node(refusal(`idRepository.getIdentity("u1").${member}()`)),
      given,
      expect: { outcome: "true", allowUndeclared: { sharedState: true, openidmReads: true } },
    }));
    if (has) {
      const error = await run.then(() => null, (e: unknown) => e);
      expect(error).toBeInstanceOf(CaseRunError);
      expect(((error as CaseRunError).response.emulation?.faults ?? []).map((f) => f.id)).toEqual([id]);
    } else {
      const { effects, verdict } = await run;
      expect(verdict.pass, verdict.summary).toBe(true);
      expect(String(effects.sharedState.final.out).split(" in object ")[0]).toBe(`TypeError: Cannot find function ${member}`);
    }
  }, 60_000);

  it("a request map's getClass() is the class refusal", async () => {
    const { effects, verdict } = await runCase(runner, defineCase({
      name: "request map getClass",
      script: node(`return [(function () { ${refusal("requestHeaders.getClass()")} })(), (function () { ${refusal("requestParameters.getClass()")} })()].join("|");`),
      given,
      expect: { outcome: "true", allowUndeclared: { sharedState: true } },
    }));
    expect(verdict.pass, verdict.summary).toBe(true);
    const refused = 'InternalError: Access to Java class "java.lang.Class" is prohibited.';
    expect(effects.sharedState.final.out).toBe(`${refused}|${refused}`);
  }, 60_000);

  it("the guard is not a member a script can see, and a script's own object is untouched", async () => {
    const { effects, verdict } = await runCase(runner, defineCase({
      name: "guard is hidden",
      script: node(`var mine = {}; var e; try { mine.frobnicate(); } catch (x) { e = x.name; } var keys = []; for (var k in utils) { keys.push(k); } ` +
        `return [e, keys.indexOf("__noSuchMethod__"), Object.keys(openidm).indexOf("__noSuchMethod__"), typeof openidm.frobnicate].join("|");`),
      expect: { outcome: "true", allowUndeclared: { sharedState: true } },
    }));
    expect(verdict.pass, verdict.summary).toBe(true);
    expect(effects.sharedState.final.out).toBe("TypeError|-1|-1|undefined");
  }, 60_000);
});
