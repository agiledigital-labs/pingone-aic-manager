/**
 * Sol r2 P2-3: what an IDM script sees when it catches an emulated CREST error, compared field for
 * field with the live captures (probe-pol and probe-def, sandbox, 2026-11-08; docs/api/10). The
 * `err` function is the probes' own capture code, verbatim. Members no probe read fault
 * `idm-exception/unmeasured` instead of answering for the emulator's own exception class.
 */
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { expectHarnessFault } from "../../src/harness/emulation-guard.ts";
import { IdmConfig, IdmEmulator, type ManagedType } from "../../src/idm/index.ts";
import { RhinoRunner } from "../../src/runner.ts";

const ERR = `function err(e) {
  var o = {name: String(e.name), message: String(e.message), str: String(e)};
  try { o.jclass = String(e.javaException.getClass().getName()); } catch (x) { o.jclass = "n/a: " + x; }
  try { o.code = Number(e.javaException.getCode()); } catch (x) { o.code = "n/a"; }
  try { o.detail = String(e.javaException.getDetail()); } catch (x) { o.detail = "n/a"; }
  try { o.reason = String(e.javaException.getReason()); } catch (x) { o.reason = "n/a"; }
  return o;
}`;
// getClass faults on read (sol-e15ab r6), so the field comparisons use the capture without its jclass
// line; the class is still compared, through e.message. The verbatim ERR is driven below and must fault.
const ERR_FIELDS = ERR.replace(/^ {2}try \{ o\.jclass = .*\n/m, "");

// The probes' shared properties (probe-pol note.md); zzemu_pol has required: [], zzemu_pol2 no required key.
const properties = { text: { type: "string" }, zready: { type: "boolean", default: true } };
const pol: ManagedType = { name: "zzemu_pol", schema: { required: [], properties } };
const pol2: ManagedType = { name: "zzemu_pol2", schema: { properties } };

const forbidden = (detail: string) => ({
  name: "JavaException",
  message: "org.forgerock.json.resource.ForbiddenException: Policy validation failed",
  str: "JavaException: org.forgerock.json.resource.ForbiddenException: Policy validation failed",
  code: 403,
  detail,
  reason: "Forbidden",
});

describe("a caught emulated CREST error matches the captured observation", () => {
  let runner: RhinoRunner;
  beforeAll(async () => { runner = await RhinoRunner.spawn(); }, 60_000);
  afterAll(async () => { await runner?.close(); });
  const observe = async (call: string) => {
    const emu = new IdmEmulator(new IdmConfig({ sync: { mappings: [] }, managed: { objects: [pol, pol2] } }), runner);
    const response = await runner.eval({ engine: "idm", source: `${ERR_FIELDS} var out; try { ${call}; out = "no error"; } catch (e) { out = err(e); } out;` },
      { onCall: emu.onCall });
    expect(response.error).toBeNull();
    return response.value;
  };

  // `reason` was not read on the 403s (probe-pol's err had no getReason line); "Forbidden" is CREST's phrase, assumed.
  it.each([
    ["probe-pol NEG-text-num", "openidm.create('managed/zzemu_pol', null, {text: 5})",
      '{ "result": false, "failedPolicyRequirements": [ { "policyRequirements": [ { "params": { "invalidType": "number", "validTypes": [ "string" ] }, "policyRequirement": "VALID_TYPE" } ], "property": "text" } ] }'],
    ["probe-pol X-zready-null", "openidm.create('managed/zzemu_pol', null, {zready: null})",
      '{ "result": false, "failedPolicyRequirements": [ { "policyRequirements": [ { "policyRequirement": "NOT_NULL" } ], "property": "zready" } ] }'],
    ["probe-pol2 Q1", "openidm.create('managed/zzemu_pol2', null, {text: null})",
      '{ "result": false, "failedPolicyRequirements": [ { "policyRequirements": [ { "params": { "invalidType": "null", "validTypes": [ "string" ] }, "policyRequirement": "VALID_TYPE" } ], "property": "text" } ] }'],
  ])("%s", async (_name, call, detail) => {
    expect(await observe(call)).toEqual(forbidden(detail));
  });

  // Discriminating: before sol-r2 the message named "HostCalls$HostException" and detail was the multi-line pretty form.
  // The capture was a delete on a type IDM could not serve (zzemu_def5); the emulator faults an undeclared
  // type's route, so the comparable 404 is a missing record. The class, code, detail and reason fields are
  // compared exactly; the message names a record rather than a type, and is the emulator's own.
  it("probe-def's 404: NotFoundException, reason Not Found, and a Java-null detail", async () => {
    const observed = await observe("openidm.delete('managed/zzemu_pol/zzemu-nope', null)") as Record<string, unknown>;
    const { name, code, detail, reason } = observed;
    expect({ name, code, detail, reason }).toEqual({
      name: "JavaException", code: 404, detail: "null", reason: "Not Found",
    });
    expect(observed.str).toBe(`JavaException: ${String(observed.message)}`);
    expect(observed.message).toMatch(/^org\.forgerock\.json\.resource\.NotFoundException: /);
  });

  it.each([
    ["another Java member", "e.javaException.getMessage()"],
    ["a bean property", "e.javaException.code"],
    ["an empty detail container", "String(e.javaException.getDetail())"],
    // sol-r3 P2-2: membership and enumeration used to answer from the local HostException (true, its
    // whole member list) or from the shim's empty inventory (false), with no fault.
    ["`in` for a member no probe read", "'getMessage' in e.javaException"],
    ["`in` for an index", "0 in e.javaException"],
    ["hasOwnProperty for a member no probe read", "Object.prototype.hasOwnProperty.call(e.javaException, 'getMessage')"],
    ["Object.keys", "Object.keys(e.javaException)"],
    ["for…in", "(function () { var k = []; for (var x in e.javaException) k.push(x); return k; })()"],
    ["JSON.stringify", "JSON.stringify(e.javaException)"],
    // sol-e15ab r6: reading getClass faults, so neither the class nor the method object is observable.
    ["reading getClass without calling it", "e.javaException.getClass"],
    ["the measured idiom getClass().getName()", "String(e.javaException.getClass().getName())"],
    ["the probes' own capture function", "err(e)"],
  ])("%s faults idm-exception/unmeasured", async (_name, read) => {
    expectHarnessFault("idm-exception/unmeasured");
    const emu = new IdmEmulator(new IdmConfig({ sync: { mappings: [] }, managed: { objects: [pol] } }), runner);
    // A 400 on an unknown action carries `{}` as its detail, whose one-line spelling no probe showed.
    const call = read.includes("getDetail") ? "openidm.action('managed/zzemu_pol', 'unknown', {})" : "openidm.create('managed/zzemu_pol', null, {text: 5})";
    const response = await runner.eval({ engine: "idm", source: `${ERR} var out; try { ${call}; } catch (e) { out = ${read}; } out;` }, { onCall: emu.onCall });
    expect(response.outcome).not.toBe("ok");
    expect(JSON.stringify(response)).toContain("idm-exception/unmeasured");
  });

  // sol-r3 P2-2: a name a read admits answers `in` truthfully (the member exists on AIC's object
  // too), and the direct reads still work, so the gate is consistent between read and membership.
  it("`in` is true for every admitted member and for getClass", async () => {
    const emu = new IdmEmulator(new IdmConfig({ sync: { mappings: [] }, managed: { objects: [pol] } }), runner);
    const response = await runner.eval({ engine: "idm", source: `var out; try { openidm.create('managed/zzemu_pol', null, {text: 5}); } catch (e) {
      var j = e.javaException;
      out = ['getCode', 'getDetail', 'getReason', 'toString', 'getClass'].map(function (n) { return n in j; })
        .concat([Number(j.getCode())]); } out;` }, { onCall: emu.onCall });
    expect(response.error).toBeNull();
    expect(response.value).toEqual([true, true, true, true, true, 403]);
  });

  // Object.getOwnPropertyNames and getOwnPropertyDescriptor never reach the view: Rhino refuses them
  // with a TypeError for every Java object, before asking it. The class shim is a plain Scriptable so it
  // gets the same refusal; the old ScriptableObject shim answered an empty list. Neither lists members.
  it.each([
    ["Object.getOwnPropertyNames", "Object.getOwnPropertyNames(e.javaException)"],
    ["Object.getOwnPropertyDescriptor", "Object.getOwnPropertyDescriptor(e.javaException, 'getCode')"],
  ])("%s is Rhino's TypeError for a Java object, not an inventory", async (_name, read) => {
    const emu = new IdmEmulator(new IdmConfig({ sync: { mappings: [] }, managed: { objects: [pol] } }), runner);
    const response = await runner.eval({ engine: "idm", source: `var out; try { openidm.create('managed/zzemu_pol', null, {text: 5}); } catch (e) {
      try { out = ${read}; out = "answered: " + JSON.stringify(out); } catch (x) { out = String(x); } } out;` }, { onCall: emu.onCall });
    expect(response.error).toBeNull();
    expect(response.value).toBe("TypeError: Expected argument of type object, but instead had type object");
  });
});
