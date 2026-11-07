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
import { create, trace, traced } from "./fixtures.ts";

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
    // Fix round 2, P3-3. Discriminating: at a825992 a removed property reached the type check as
    // `undefined` and faulted valid-type ("the write sends undefined"). Stock refuses nothing here.
    ["patch removes an undefaulted optional property", gadget([]), { count: 1 },
      { creating: false, patchedFrom: { text: "a", count: 1 }, operations: [{ operation: "remove", field: "/text" }] }, "assumed"],
    ["patch removes an untyped property", gadget([]), { count: 1 },
      { creating: false, patchedFrom: { loose: "x", count: 1 }, operations: [{ operation: "remove", field: "/loose" }] }, "assumed"],
    // Fix round 2, P3-1: stock adds an implicit required for any default; unmeasured without schema.required.
    ["patch removes a defaulted property", gadget([]), { text: "a" },
      { creating: false, patchedFrom: { text: "a", count: 1 }, operations: [{ operation: "remove", field: "/count" }] }, "fault", "idm-policy/required"],
    ["patch removes a null-defaulted property", gadget([]), { text: "a" },
      { creating: false, patchedFrom: { text: "a", nothing: "x" }, operations: [{ operation: "remove", field: "/nothing" }] }, "fault", "idm-policy/required"],
  ];
  it.each(cases)("%s", (_name, type, object, write, kind, id) => {
    const verdict = implicitPolicy(type, object, write);
    expect(verdict.kind).toBe(kind);
    if (verdict.kind === "fault") expect(verdict.id).toBe(id);
    if (verdict.kind === "notNull") expect(verdict.property).toBe("count");
  });

  // Discriminating: an untyped value would also fail a plain `actual !== type` comparison, so only
  // the reason tells a missing type from a mismatch, for a value and for a null alike.
  it.each([["a value", "x"], ["a null", null]] as const)("an untyped property says it declares no type, for %s", (_name, value) => {
    const verdict = implicitPolicy(gadget([]), { loose: value }, creating);
    expect(verdict).toMatchObject({ kind: "fault", id: "idm-policy/valid-type", reason: expect.stringContaining("declares no type") });
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

  it("an explicit null on a defaulted property is the measured envelope and requirement (single-requirement shape from the stock reading)", async () => {
    const result = await make(gadget([])).request({ ...createPath, body: { count: null } });
    expect(result.status).toBe(403);
    // doc 10's verbatim REQUIRED envelope with NOT_NULL on the property (measured); that NOT_NULL is
    // the only requirement listed is the stock policy.js reading (assumed).
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

  // P3-3: the create chain runs before the collision (doc 24), but the measured NOT_NULL was a new
  // record; a create-first write over an existing one is not that case, whichever route reaches it.
  it.each([
    ["an unconditional PUT", { method: "PUT", path: "managed/gadget/g4", body: { count: null } }],
    ["a create on an existing id", { ...createPath, body: { _id: "g4", count: null } }],
  ] as const)("a defaulted null through %s over an existing record faults rather than emulating the 403", async (_name, request) => {
    expectHarnessFault("idm-policy/not-null");
    const emu = make(gadget([]));
    expect((await emu.request({ method: "PUT", path: "managed/gadget/g4", body: { count: 1 } })).status).toBe(201);
    const result = await emu.request(request);
    expect(result.status).toBe(501);
    expect(JSON.stringify(result.body)).toContain("idm-policy/not-null");
    expect((await emu.request({ method: "GET", path: "managed/gadget/g4" })).body).toMatchObject({ count: 1 });
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

describe("PATCH replace with null is the measured 400, whatever the property (P2-1)", () => {
  let runner: RhinoRunner;
  beforeAll(async () => { runner = await RhinoRunner.spawn(); }, 60_000);
  afterAll(async () => { await runner?.close(); });
  const seeded = async (required?: string[]) => {
    const type = gadget(required);
    const { nothing: _nothing, ...properties } = type.schema.properties;
    const emu = new IdmEmulator(new IdmConfig({ sync: { mappings: [] }, managed: { objects: [{ ...type, schema: { ...type.schema, properties } }] } }), runner);
    expect((await emu.request({ method: "PUT", path: "managed/gadget/p1", body: { text: "a", count: 1, tags: ["x"] } })).status).toBe(201);
    return emu;
  };
  const patch = (operations: Document[]) => ({ method: "PATCH", path: "managed/gadget/p1", body: operations }) as const;

  it.each([
    // Discriminating: undefaulted, required: [] is the path that used to store null and answer 200.
    ["an undefaulted property with a required array", [], [{ operation: "replace", field: "/text", value: null }]],
    ["an undefaulted property with no required array", undefined, [{ operation: "replace", field: "/text", value: null }]],
    ["a defaulted property", [], [{ operation: "replace", field: "/count", value: null }]],
    ["an array element", [], [{ operation: "replace", field: "/tags/0", value: null }]],
    // A parse-time refusal judges the whole patch before applying any of it.
    ["a later operation, after one that would fail to apply", [], [
      { operation: "replace", field: "/box/inner", value: 1 }, { operation: "replace", field: "/text", value: null }]],
  ] as const)("on %s", async (_name, required, operations) => {
    const emu = await seeded(required as string[] | undefined);
    const result = await emu.request(patch(operations as unknown as Document[]));
    expect(result.status).toBe(400);
    expect(String((result.body as Document).message)).toContain("not a valid JSON patch");
    expect(result.provenance).toContainEqual(expect.objectContaining({ kind: "measured",
      behavior: "PATCH replace with null is refused 400 as an invalid JSON patch, before policy" }));
    expect((await emu.request({ method: "GET", path: "managed/gadget/p1" })).body).toMatchObject({ text: "a", count: 1 });
  });

  it("add with null is unmeasured and faults idm-patch/add-null", async () => {
    expectHarnessFault("idm-patch/add-null");
    const emu = await seeded([]);
    const result = await emu.request(patch([{ operation: "add", field: "/text", value: null }]));
    expect(result.status).toBe(501);
    expect(JSON.stringify(result.body)).toContain("idm-patch/add-null");
  });

  // Fix round 2, P2-1: a script's undefined reaches IDM as null (docs/api/12).
  it("a script's openidm.patch with value undefined gets the same 400", async () => {
    const emu = await seeded([]);
    const result = await runner.eval({ engine: "idm", source:
      "var out; try { openidm.patch('managed/gadget/p1', null, [{operation: 'replace', field: '/text', value: undefined}]); out = 'patched'; } " +
      "catch (e) { out = String(e); } out;" }, { onCall: emu.onCall });
    expect(String(result.value)).toContain("not a valid JSON patch");
  });

  // Fix round 2, P3-2: the refusal is parse-time, ahead of lookup, revision and capability gate.
  // Discriminating: at a825992 these answered 404 and 412.
  it.each([
    ["an absent record", { method: "PATCH", path: "managed/gadget/absent" }],
    ["a stale revision", { method: "PATCH", path: "managed/gadget/p1", headers: { "If-Match": "9" } }],
  ] as const)("answers before %s", async (_name, request) => {
    const emu = await seeded([]);
    const result = await emu.request({ ...request, body: [{ operation: "replace", field: "/text", value: null }] });
    expect(result.status).toBe(400);
    expect(String((result.body as Document).message)).toContain("not a valid JSON patch");
  });
  it("a well-formed patch on an absent record is still 404 (positive control)", async () => {
    const emu = await seeded([]);
    expect((await emu.request({ method: "PATCH", path: "managed/gadget/absent", body: [{ operation: "replace", field: "/text", value: "b" }] })).status).toBe(404);
  });
  // The measured has-one relationship case is now reachable: the refusal comes before the
  // capability gate that faults every write on a relationship type.
  it("answers on a type with a relationship property, which the capability gate would fault", async () => {
    const emu = new IdmEmulator(new IdmConfig({ sync: { mappings: [] }, managed: { objects: [{ name: "linked", schema: { properties: {
      text: { type: "string" }, owner: { type: "relationship", resourceCollection: [{ path: "managed/linked" }] } } } }] } }), runner);
    const result = await emu.request({ method: "PATCH", path: "managed/linked/x", body: [{ operation: "replace", field: "/owner", value: null }] });
    expect(result.status).toBe(400);
  });

  // Discriminating: at a825992 onRetrieve and onRead ran (and wrote trace records) before the 400.
  it("answers before any read hook runs", async () => {
    const emu = traced(runner);
    await create(emu);
    const before = (await trace(emu)).length;
    const result = await emu.request({ method: "PATCH", path: "managed/widget/one", body: [{ operation: "replace", field: "/text", value: null }] });
    expect(result.status).toBe(400);
    expect((await trace(emu)).length).toBe(before);
  });

  // Fix round 2, P3-3 end to end: removing an optional undefaulted property commits.
  it("remove of an undefaulted optional property commits", async () => {
    const emu = await seeded([]);
    const result = await emu.request(patch([{ operation: "remove", field: "/text" }]));
    expect(result.status).toBe(200);
    expect(result.provenance).toContainEqual(expect.objectContaining({ kind: "assumed", behavior: ASSUMED_POLICY }));
    expect((await emu.request({ method: "GET", path: "managed/gadget/p1" })).body).toMatchObject({ text: null, count: 1 });
  });
  it("remove of a defaulted property faults idm-policy/required", async () => {
    expectHarnessFault("idm-policy/required");
    const emu = await seeded([]);
    const result = await emu.request(patch([{ operation: "remove", field: "/count" }]));
    expect(result.status).toBe(501);
    expect(JSON.stringify(result.body)).toContain("idm-policy/required");
    expect((await emu.request({ method: "GET", path: "managed/gadget/p1" })).body).toMatchObject({ count: 1 });
  });

  it("a script's openidm.patch gets the same 400", async () => {
    const emu = await seeded([]);
    const result = await runner.eval({ engine: "idm", source:
      "var out; try { openidm.patch('managed/gadget/p1', null, [{operation: 'replace', field: '/text', value: null}]); out = 'patched'; } " +
      "catch (e) { out = String(e); } out;" }, { onCall: emu.onCall });
    expect(result.outcome).toBe("ok");
    expect(String(result.value)).toContain("not a valid JSON patch");
  });
});
