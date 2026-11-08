/**
 * An injected openidm failure's `e.javaException.getDetail()`, on the real JVM,
 * compared with the live capture (openidm-error-detail probe, sandbox,
 * 2026-11-08; docs/api/12). The script is the probe fixture itself, verbatim,
 * with only its `emit` replaced; each of its three calls is failed by a stub
 * carrying the measured message and detail.
 */
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { parseHarvest } from "../../src/bindings/harvest.ts";
import { mockPreamble, withHarvest } from "../../src/bindings/preamble.ts";
import type { Given } from "../../src/case/types.ts";
import { validateCase } from "../../src/case/validate.ts";
import { expectHarnessFault } from "../../src/harness/emulation-guard.ts";
import { repoRoot } from "../../src/paths.ts";
import { RhinoRunner } from "../../src/runner.ts";

const probe = readFileSync(join(repoRoot, "scripts/rhino-script-tester/fixtures/openidm-error-detail.script.js"), "utf8");
// The probe names its throwaway users with randomUUID; pin it so the host and
// container lanes see the same ids and the both-lane comparison stays exact.
const pinUuid = 'var __uuidN = 0; utils.crypto.randomUUID = function () { __uuidN++; return "00000000-0000-4000-8000-" + ("000000000000" + __uuidN).slice(-12); };\n';
const capture = `${pinUuid}${probe}\nfunction emit(payload) { nodeState.putShared("r", JSON.stringify(payload)); }`;

const ADAPTER = "org.forgerock.openam.scripting.wrappers.ResourceExceptionScriptAdapter: ";
const POLICY = { result: false, failedPolicyRequirements: [{ policyRequirements: [{ policyRequirement: "NOT_NULL" }], property: "frIndexedInteger1" }] };
const POLICY_TEXT = '{ "result": false, "failedPolicyRequirements": [ { "policyRequirements": [ { "policyRequirement": "NOT_NULL" } ], "property": "frIndexedInteger1" } ] }';
// Object.keys / for...in on AIC's JsonValue, in the captured order.
const MEMBERS = "getClass,equalTo,setObject,put,asList,required,patch,number,putPermissive,iterator,putIfNotNull,isNumber,merge,asString,defined,isNotNull,add,getObject,pointer,spliterator,notNull,asMapOfList,asAsync,diff,collection,list,isMap,asInteger,asLong,contains,as,size,null,asDouble,asBoolean,object,wait,string,keys,asMap,notifyAll,isCollection,isList,notify,remove,isBoolean,stream,addIfNotNull,hashCode,get,isString,copy,class,map,asCollection,forEach,asNumber,clear,defaultTo,getPointer,expect,boolean,addPermissive,isDefined,equals,isNull,clone,toString,isEqualTo";

const detail = (str: string, getResult: string, jvIsNull: boolean, getObject: string) => ({
  typeof: "object", isNull: false, isUndefined: false, str, json: JSON.stringify(str), typeofGet: "function", getResult, jvIsNull, getObject,
  keys: MEMBERS, forIn: MEMBERS, dotResult: "undefined:undefined", dotFpr: "undefined",
});
const failure = (message: string, code: number, reason: string, d: unknown) => ({
  threw: true, name: "JavaException", message: ADAPTER + message, hasJ: "object", typeofGetDetail: "function", inGetDetail: true, code, reason, detail: d,
});
const NOT_FOUND = "No Such Entry: the search base entry does not exist";
const BAD_ACTION = "Expecting String containing one of: patch triggerSyncCheck updateLastSync";
const given: Given = {
  managed: { "managed/bravo_user": [], "managed/alpha_user": [] },
  openidmFailures: [
    { match: { method: "create", resource: /^managed\/bravo_user\//, ordinal: 1 }, reply: { code: 403, message: "Policy validation failed", detail: POLICY } },
    { match: { method: "delete", resource: /^managed\/alpha_user\//, ordinal: 1 }, reply: { code: 404, message: NOT_FOUND, detail: null } },
    { match: { method: "action", resource: "managed/alpha_user", ordinal: 1 }, reply: { code: 400, message: BAD_ACTION, detail: null } },
  ],
};

describe("an injected failure's getDetail matches the live capture", () => {
  let runner: RhinoRunner;
  beforeAll(async () => { runner = await RhinoRunner.spawn(); }, 60_000);
  afterAll(async () => { await runner?.close(); }, 30_000);

  it("policy 403 object, 404 and 400 null", async () => {
    const response = await runner.eval({ preamble: mockPreamble(given), source: withHarvest(capture) });
    expect(response.outcome, JSON.stringify(response.error)).toBe("ok");
    const out = JSON.parse(parseHarvest(response.value as string).sharedState.final.r as string) as Record<string, unknown>;
    expect(out).toEqual({
      ok: true,
      feature: "openidm-error-detail",
      policy: failure("Policy validation failed", 403, "Forbidden", detail(POLICY_TEXT, "false", false, JSON.stringify(POLICY))),
      notFound: failure(NOT_FOUND, 404, "Not Found", detail("null", "null", true, "null")),
      badAction: failure(BAD_ACTION, 400, "Bad Request", detail("null", "null", true, "null")),
    });
  });

  it.each([
    ["a stub with no detail", undefined, "String(e.javaException.getDetail())"],
    ["an unmeasured member", null, "e.javaException.getDetail().asMap"],
    ["an empty object's string form", {}, "String(e.javaException.getDetail())"],
  ])("%s faults openidm-failure-detail, even when caught", async (_name, d, read) => {
    expectHarnessFault("openidm-failure-detail");
    const reply = d === undefined ? { code: 404 } : { code: 404, detail: d };
    const stubbed: Given = { managed: { "managed/widget": [] }, openidmFailures: [{ match: { method: "delete", resource: "managed/widget/w1", ordinal: 1 }, reply }] };
    const response = await runner.eval({ preamble: mockPreamble(stubbed), source: withHarvest(`try { openidm.delete("managed/widget/w1", null); } catch (e) { try { ${read}; } catch (x) {} }`) });
    expect(response.emulation?.faults?.map((f) => f.id)).toEqual(["openidm-failure-detail"]);
  });
});

describe("openidmFailures reply validation", () => {
  const stub = (reply: unknown) => ({ name: "c", outcomes: ["true"], script: "action.goTo(\"true\");", given: { openidmFailures: [{ match: { method: "read", resource: "managed/x/1", ordinal: 1 }, reply }] }, expect: { outcome: "true" } });
  it("accepts detail (null included) and message", () => {
    expect(() => validateCase(stub({ code: 404, detail: null, message: "gone" }))).not.toThrow();
  });
  it.each([
    ["an empty message", { code: 404, message: "" }, /reply\.message/],
    ["an unknown key", { code: 404, reason: "Gone" }, /reason/],
    ["a non-JSON detail", { code: 404, detail: () => 0 }, /reply\.detail/],
  ])("refuses %s", (_name, reply, error) => {
    expect(() => validateCase(stub(reply))).toThrow(error);
  });
});
