/**
 * E15a: a write with no PolicyEvaluator. The measured NOT_NULL is emulated, unmeasured implicit
 * policy outcomes fault under `idm-policy/*`, and only type-valid writes keep the assumption.
 * Schemas are invented.
 */
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { expectHarnessFault } from "../../src/harness/emulation-guard.ts";
import { IdmConfig, IdmEmulator, type Document, type ManagedType } from "../../src/idm/index.ts";
import { ASSUMED_POLICY, type ImplicitPolicyVerdict, implicitPolicy, type PolicyWrite } from "../../src/idm/policy.ts";
import { RhinoRunner } from "../../src/runner.ts";

const gadget = (required?: string[]): ManagedType => ({ name: "gadget", schema: {
  ...(required ? { required } : {}),
  properties: {
    text: { type: "string" }, count: { type: "number", default: 0 }, flag: { type: "boolean", default: false },
    tags: { type: "array", items: { type: "string" } }, loose: {}, nothing: { type: "string", default: null },
    box: { type: "object", properties: { inner: { type: "number" } } },
  },
} });
const creating: PolicyWrite = { creating: true };

describe("implicitPolicy decides from the real schema", () => {
  const cases: [string, ManagedType, Document, PolicyWrite, ImplicitPolicyVerdict["kind"], string?][] = [
    // The measured case: a defaulted property, a new record, a required array.
    ["defaulted null on create", gadget([]), { count: null }, creating, "notNull"],
    // Discriminating: the measured null always had a default. No default and an optional
    // property is what stock policy.js accepts, so `{text: null}` must not become NOT_NULL.
    ["undefaulted optional null", gadget([]), { text: null }, creating, "assumed"],
    ["undefaulted null, no required array", gadget(), { text: null }, creating, "fault", "idm-policy/valid-type"],
    ["default: null is no default", gadget([]), { nothing: null }, creating, "assumed"],
    ["defaulted null, no required array", gadget(), { count: null }, creating, "fault", "idm-policy/not-null"],
    ["defaulted null over an existing record", gadget([]), { count: null }, { creating: false }, "fault", "idm-policy/not-null"],
    ["two defaulted nulls", gadget([]), { count: null, flag: null }, creating, "fault", "idm-policy/not-null"],
    ["a fault beats an emulated null", gadget([]), { count: null, text: 5 }, creating, "fault", "idm-policy/valid-type"],
    ["string sent as number", gadget([]), { text: 5 }, creating, "fault", "idm-policy/valid-type"],
    ["number sent as string", gadget([]), { count: "7" }, creating, "fault", "idm-policy/valid-type"],
    ["array sent as object", gadget([]), { tags: {} }, creating, "fault", "idm-policy/valid-type"],
    ["untyped property written", gadget([]), { loose: "x" }, creating, "fault", "idm-policy/valid-type"],
    ["nested mismatch", gadget([]), { box: { inner: "x" } }, creating, "fault", "idm-policy/valid-type"],
    ["nested null", gadget([]), { box: { inner: null } }, creating, "fault", "idm-policy/valid-type"],
    ["array item mismatch", gadget([]), { tags: ["a", 2] }, creating, "fault", "idm-policy/valid-array-items"],
    ["array item null", gadget([]), { tags: [null] }, creating, "fault", "idm-policy/valid-array-items"],
    ["type-valid write", gadget(), { text: "a", count: 1, flag: true, tags: ["a"], box: { inner: 2 } }, creating, "assumed"],
    // PATCH policy covers what the patch writes (doc 10's enum table): a stale bad value elsewhere passes.
    ["patch leaves a stale value alone", gadget([]), { text: 5, count: 2 },
      { creating: false, patchedFrom: { text: 5, count: 1 }, operations: [{ operation: "replace", field: "/count", value: 2 }] }, "assumed"],
    ["patch writes a bad value", gadget([]), { text: 6, count: 1 },
      { creating: false, patchedFrom: { text: "a", count: 1 }, operations: [{ operation: "replace", field: "/text", value: 6 }] }, "fault", "idm-policy/valid-type"],
    ["patch names a property it leaves equal", gadget(), { text: null },
      { creating: false, patchedFrom: { text: null }, operations: [{ operation: "replace", field: "/text", value: null }] }, "fault", "idm-policy/valid-type"],
    ["a hook's change counts as written", gadget([]), { text: 7 },
      { creating: false, patchedFrom: { text: "a" }, operations: [] }, "fault", "idm-policy/valid-type"],
  ];
  it.each(cases)("%s", (_name, type, object, write, kind, id) => {
    const verdict = implicitPolicy(type, object, write);
    expect(verdict.kind).toBe(kind);
    if (verdict.kind === "fault") expect(verdict.id).toBe(id);
    if (verdict.kind === "notNull") expect(verdict.property).toBe("count");
  });
});

describe("the emulator applies it to writes without a policy evaluator", () => {
  let runner: RhinoRunner;
  beforeAll(async () => { runner = await RhinoRunner.spawn(); }, 60_000);
  afterAll(async () => { await runner?.close(); });
  // Config validation already refuses a `default: null` on a string, so the emulator's schema
  // drops that one property; the unit table above covers it.
  const make = (type: ManagedType, options: ConstructorParameters<typeof IdmEmulator>[2] = {}) => {
    const { nothing: _nothing, ...properties } = type.schema.properties;
    return new IdmEmulator(new IdmConfig({ sync: { mappings: [] }, managed: { objects: [{ ...type, schema: { ...type.schema, properties } }] } }), runner, options);
  };
  const createPath = { method: "POST", path: "managed/gadget", query: { _action: "create" } } as const;

  it("an explicit null on a defaulted property is the measured 403, byte for byte", async () => {
    const result = await make(gadget([])).request({ ...createPath, body: { count: null } });
    expect(result.status).toBe(403);
    // doc 10's verbatim REQUIRED envelope, with NOT_NULL on the property.
    expect(JSON.stringify(result.body)).toBe('{"code":403,"reason":"Forbidden","message":"Policy validation failed",' +
      '"detail":{"result":false,"failedPolicyRequirements":[{"policyRequirements":[{"policyRequirement":"NOT_NULL"}],"property":"count"}]}}');
    expect(result.provenance).toContainEqual(expect.objectContaining({ kind: "measured",
      behavior: "explicit null on a defaulted property is refused NOT_NULL on create" }));
  });

  it("a script's caught openidm.create gets the same refusal, and nothing is stored", async () => {
    const emu = make(gadget([]));
    const result = await runner.eval({ engine: "idm", source:
      "var out; try { openidm.create('managed/gadget', 'g1', {count: null}); out = 'stored'; } " +
      "catch (e) { out = String(e); } out;" }, { onCall: emu.onCall });
    expect(result.outcome).toBe("ok");
    expect(String(result.value)).toContain("Policy validation failed");
    const read = await emu.request({ method: "GET", path: "managed/gadget/g1" });
    expect(read.status).toBe(404);
  });

  it("a type mismatch is the harness fault idm-policy/valid-type and does not write", async () => {
    expectHarnessFault("idm-policy/valid-type");
    const emu = make(gadget([]));
    const result = await emu.request({ ...createPath, body: { _id: "g2", text: 5 } });
    expect(result.status).toBe(501);
    expect(JSON.stringify(result.body)).toContain("idm-policy/valid-type");
    expect((await emu.request({ method: "GET", path: "managed/gadget/g2" })).status).toBe(404);
  });

  it("an array item mismatch is the harness fault idm-policy/valid-array-items", async () => {
    expectHarnessFault("idm-policy/valid-array-items");
    const result = await make(gadget([])).request({ ...createPath, body: { tags: ["a", 1] } });
    expect(result.status).toBe(501);
  });

  it("a null over an existing record is the harness fault idm-policy/not-null", async () => {
    expectHarnessFault("idm-policy/not-null");
    const emu = make(gadget([]));
    expect((await emu.request({ method: "PUT", path: "managed/gadget/g3", body: { count: 1 } })).status).toBe(201);
    const result = await emu.request({ method: "PUT", path: "managed/gadget/g3", headers: { "If-Match": "*" }, body: { count: null } });
    expect(result.status).toBe(501);
  });

  it("a type-valid write keeps the reworded assumption and commits", async () => {
    const result = await make(gadget([])).request({ ...createPath, body: { text: "a", count: 2, tags: ["x"] } });
    expect(result.status).toBe(201);
    expect(result.provenance).toContainEqual({ kind: "assumed", behavior: ASSUMED_POLICY });
  });

  it("a supplied evaluator replaces the implicit check entirely", async () => {
    const result = await make(gadget([]), { policy: () => {} }).request({ ...createPath, body: { text: 5 } });
    expect(result.status).toBe(201);
  });
});
