/**
 * E15a: a write with no PolicyEvaluator. The measured NOT_NULL is emulated, unmeasured implicit
 * policy outcomes fault under `idm-policy/*`, and only type-valid writes keep the assumption.
 * Schemas are invented.
 */
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { expectHarnessFault } from "../../src/harness/emulation-guard.ts";
import { IdmConfig, IdmEmulator, type Document, type ManagedType } from "../../src/idm/index.ts";
import { ASSUMED_POLICY, type ImplicitPolicyVerdict, implicitPolicy, NESTED_NULL, POLICY_MEASURED, type PolicyWrite } from "../../src/idm/policy.ts";
import { RhinoRunner } from "../../src/runner.ts";
import { create, trace, traced } from "./fixtures.ts";

const gadget = (required?: string[]): ManagedType => ({ name: "gadget", schema: {
  ...(required ? { required } : {}),
  properties: {
    text: { type: "string" }, count: { type: "number", default: 0 }, flag: { type: "boolean", default: false },
    tags: { type: "array", items: { type: "string" } }, loose: {}, nothing: { type: "string", default: null },
    box: { type: "object", properties: { inner: { type: "number" }, tags: { type: "array", items: { type: "string" } } } },
    rows: { type: "array", items: { type: "object", properties: { inner: { type: "number" } } } },
  },
} });
const creating: PolicyWrite = { creating: true };
// sol-r2 (zzemu_nd, 2026-11-08): an integer beside a number, and null defaults on non-string types.
const widget = (required?: string[]): ManagedType => ({ name: "widget", schema: {
  ...(required ? { required } : {}),
  properties: { i: { type: "integer" }, n: { type: "number" }, list: { type: "array", items: { type: "string" }, default: null },
    flag: { type: "boolean", default: null }, text: { type: "string", default: null } },
} });

const nn = (property: string) => ({ policyRequirements: [{ policyRequirement: "NOT_NULL" }], property });
const vt = (property: string, invalidType: string, declared: string) =>
  ({ policyRequirements: [{ params: { invalidType, validTypes: [declared] }, policyRequirement: "VALID_TYPE" }], property });

describe("implicitPolicy decides from the real schema", () => {
  // The fifth column is the fault id, or for a refusal the failedPolicyRequirements it must carry.
  const cases: [string, ManagedType, Document, PolicyWrite, ImplicitPolicyVerdict["kind"], (string | object[])?][] = [
    // Measured (doc 10, 2026-11-08 probe): a defaulted null with a required array is NOT_NULL alone.
    ["defaulted null on create", gadget([]), { count: null }, creating, "refuse", [nn("count")]],
    // Discriminating for P2-3: the same null body against required: [] and against no required key.
    ["undefaulted null, required: []", gadget([]), { text: null }, creating, "pass"],
    ["undefaulted null, no required array", gadget(), { text: null }, creating, "refuse", [vt("text", "null", "string")]],
    // A non-empty list that omits the property behaves like the empty one (zzemu_pol3).
    ["undefaulted null, required names another property", gadget(["count"]), { text: null }, creating, "pass"],
    // With no required key a defaulted null is both entries, NOT_NULL first (measured order).
    ["defaulted null, no required array", gadget(), { flag: null }, creating, "refuse", [nn("flag"), vt("flag", "null", "boolean")]],
    // sol-r1c, zzemu_def probe D1: NOT_NULL keys on a non-null default, so default: null gives none.
    ["default: null gives no NOT_NULL (measured)", gadget([]), { nothing: null }, creating, "pass"],
    // zzemu_nd N6 (measured): with no required key a string's sent null is VALID_TYPE, its applied one stored.
    ["default: null sent with no required array", gadget(), { nothing: null }, creating, "refuse", [vt("nothing", "null", "string")]],
    ["default: null applied with no required array", gadget(), { nothing: null }, { creating: true, applied: ["nothing"] }, "pass"],
    // Unmeasured: N6 covered a string only.
    ["a boolean's default: null with no required array", widget(), { flag: null }, { creating: true, applied: ["flag"] }, "fault", "idm-policy/valid-type"],
    ["a boolean's default: null, required: [] (N1)", widget([]), { flag: null }, creating, "pass"],
    // An array's null default is [] on create (the emulator coerces it before policy); elsewhere a null faults.
    ["an array's null default over an existing record", widget([]), { list: null }, { creating: false }, "fault", "idm-policy/valid-type"],
    // Discriminating for sol-r2 P2-2: every JSON number is "number" to IDM, so 3 is refused on an integer.
    ["3 on an integer (N3)", widget([]), { i: 3 }, creating, "refuse", [vt("i", "number", "integer")]],
    ["3.5 on an integer (N3)", widget([]), { i: 3.5 }, creating, "refuse", [vt("i", "number", "integer")]],
    ["3 on a number (N2 control)", widget([]), { n: 3 }, creating, "pass"],
    ["null on an integer (N3)", widget([]), { i: null }, creating, "pass"],
    ["defaulted null over an existing record", gadget([]), { count: null }, { creating: false }, "fault", "idm-policy/not-null"],
    ["undefaulted null over an existing record, no required array", gadget(), { text: null }, { creating: false }, "fault", "idm-policy/valid-type"],
    ["undefaulted null over an existing record, required: []", gadget([]), { text: null }, { creating: false }, "pass"],
    ["two failing properties", gadget([]), { count: null, flag: null }, creating, "fault", "idm-policy/not-null"],
    ["a null and a mismatch on two properties", gadget([]), { count: null, text: 5 }, creating, "fault", "idm-policy/not-null"],
    ["two mismatches", gadget([]), { text: 5, tags: {} }, creating, "fault", "idm-policy/valid-type"],
    // Measured top-level VALID_TYPE for declared string, object and array.
    ["string sent as number", gadget([]), { text: 5 }, creating, "refuse", [vt("text", "number", "string")]],
    ["array sent as object", gadget([]), { tags: {} }, creating, "refuse", [vt("tags", "object", "array")]],
    ["object sent as string", gadget([]), { box: "s" }, creating, "refuse", [vt("box", "string", "object")]],
    // Measured for number and boolean too (zzemu_nd N1/N2).
    ["number sent as string", gadget([]), { count: "7" }, creating, "refuse", [vt("count", "string", "number")]],
    ["boolean sent as string", gadget([]), { flag: "x" }, creating, "refuse", [vt("flag", "string", "boolean")]],
    // Unmeasured writes still fault.
    ["mismatch over an existing record", gadget([]), { text: 5 }, { creating: false }, "fault", "idm-policy/valid-type"],
    ["untyped property written", gadget([]), { loose: "x" }, creating, "fault", "idm-policy/valid-type"],
    // P2-1, measured: nested schemas are not enforced, through objects and through array items.
    ["nested mismatch in an object", gadget([]), { box: { inner: "x" } }, creating, "pass"],
    ["nested array item in an object", gadget([]), { box: { tags: [2] } }, creating, "pass"],
    ["object property in array items", gadget([]), { rows: [{ inner: "bad" }] }, creating, "pass"],
    ["nested null", gadget([]), { box: { inner: null } }, creating, "pass"],
    // A top-level array's own items were not tested: still a fault.
    ["top-level array item mismatch", gadget([]), { tags: ["a", 2] }, creating, "fault", "idm-policy/valid-array-items"],
    ["top-level array item null", gadget([]), { tags: [null] }, creating, "fault", "idm-policy/valid-array-items"],
    ["type-valid write", gadget(), { text: "a", count: 1, flag: true, tags: ["a"], box: { inner: 2 }, rows: [{ inner: 1 }] }, creating, "pass"],
    // PATCH policy covers what the patch writes (doc 10's enum table): a stale bad value elsewhere passes.
    ["patch leaves a stale value alone", gadget([]), { text: 5, count: 2 },
      { creating: false, patchedFrom: { text: 5, count: 1 }, operations: [{ operation: "replace", field: "/count", value: 2 }] }, "pass"],
    ["patch writes a bad value", gadget([]), { text: 6, count: 1 },
      { creating: false, patchedFrom: { text: "a", count: 1 }, operations: [{ operation: "replace", field: "/text", value: 6 }] }, "fault", "idm-policy/valid-type"],
    ["patch names a property it leaves equal", gadget(), { text: null },
      { creating: false, patchedFrom: { text: null }, operations: [{ operation: "replace", field: "/text", value: null }] }, "fault", "idm-policy/valid-type"],
    ["a hook's change counts as written", gadget([]), { text: 7 },
      { creating: false, patchedFrom: { text: "a" }, operations: [] }, "fault", "idm-policy/valid-type"],
    // Measured 2026-11-08: removing an undefaulted, non-required property is 200, both required shapes.
    ["patch removes an undefaulted optional property", gadget([]), { count: 1 },
      { creating: false, patchedFrom: { text: "a", count: 1 }, operations: [{ operation: "remove", field: "/text" }] }, "pass"],
    ["patch removes an undefaulted optional property, no required array", gadget(), { count: 1 },
      { creating: false, patchedFrom: { text: "a", count: 1 }, operations: [{ operation: "remove", field: "/text" }] }, "pass"],
    ["patch removes an untyped property", gadget([]), { count: 1 },
      { creating: false, patchedFrom: { loose: "x", count: 1 }, operations: [{ operation: "remove", field: "/loose" }] }, "pass"],
    // Fix round 2, P3-1: stock adds an implicit required for any default; unmeasured without schema.required.
    ["patch removes a defaulted property", gadget([]), { text: "a" },
      { creating: false, patchedFrom: { text: "a", count: 1 }, operations: [{ operation: "remove", field: "/count" }] }, "fault", "idm-policy/required"],
    ["patch removes a null-defaulted property", gadget([]), { text: "a" },
      { creating: false, patchedFrom: { text: "a", nothing: "x" }, operations: [{ operation: "remove", field: "/nothing" }] }, "fault", "idm-policy/required"],
  ];
  it.each(cases)("%s", (_name, type, object, write, kind, expected) => {
    const verdict = implicitPolicy(type, object, write);
    expect(verdict.kind).toBe(kind);
    if (verdict.kind === "fault") expect(verdict.id).toBe(expected);
    if (verdict.kind === "refuse") expect(verdict.failed).toEqual(expected);
  });

  // Discriminating: an untyped value would also fail a plain `actual !== type` comparison, so only
  // the reason tells a missing type from a mismatch, for a value and for a null alike.
  it.each([["a value", "x"], ["a null", null]] as const)("an untyped property says it declares no type, for %s", (_name, value) => {
    const verdict = implicitPolicy(gadget([]), { loose: value }, creating);
    expect(verdict).toMatchObject({ kind: "fault", id: "idm-policy/valid-type", reason: expect.stringContaining("declares no type") });
  });

  // Each pass names what it relied on, so provenance says measured only where the probe covered it.
  it.each([
    ["an accepted null", gadget([]), { text: null }, creating, [POLICY_MEASURED.nullAccepted], []],
    ["a null on a null-defaulted property", gadget([]), { nothing: null }, creating, [POLICY_MEASURED.nullDefault], []],
    ["nested values", gadget([]), { rows: [{ inner: "bad" }] }, creating, [POLICY_MEASURED.nestedUnchecked], []],
    ["a nested null", gadget([]), { box: { inner: null } }, creating, [POLICY_MEASURED.nestedUnchecked], [NESTED_NULL]],
    ["an optional remove", gadget([]), {}, { creating: false, patchedFrom: { text: "a" }, operations: [{ operation: "remove", field: "/text" }] },
      [POLICY_MEASURED.optionalRemove], []],
    ["a null outside a create", gadget([]), { text: null }, { creating: false }, [], [expect.stringContaining("as measured on create")]],
  ] as const)("a pass records its evidence: %s", (_name, type, object, write, measured, assumed) => {
    expect(implicitPolicy(type, object as Document, write as PolicyWrite)).toEqual({ kind: "pass", measured, assumed });
  });
});

describe("the emulator applies it to writes without a policy evaluator", () => {
  let runner: RhinoRunner;
  beforeAll(async () => { runner = await RhinoRunner.spawn(); }, 60_000);
  afterAll(async () => { await runner?.close(); });
  // The emulator's schema drops the null-defaulted property so that `gadget()` (no required key)
  // creates are not all the unmeasured null-default fault; the null-default tests below build their own.
  const make = (type: ManagedType, options: ConstructorParameters<typeof IdmEmulator>[2] = {}) => {
    const { nothing: _nothing, ...properties } = type.schema.properties;
    return new IdmEmulator(new IdmConfig({ sync: { mappings: [] }, managed: { objects: [{ ...type, schema: { ...type.schema, properties } }] } }), runner, options);
  };
  const createPath = { method: "POST", path: "managed/gadget", query: { _action: "create" } } as const;

  it("an explicit null on a defaulted property is the measured envelope and requirement", async () => {
    const result = await make(gadget([])).request({ ...createPath, body: { count: null } });
    expect(result.status).toBe(403);
    expect(JSON.stringify(result.body)).toBe('{"code":403,"reason":"Forbidden","message":"Policy validation failed",' +
      '"detail":{"result":false,"failedPolicyRequirements":[{"policyRequirements":[{"policyRequirement":"NOT_NULL"}],"property":"count"}]}}');
    expect(result.provenance).toContainEqual({ kind: "measured", behavior: POLICY_MEASURED.notNull, source: "docs/api/10-managed-objects.md" });
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

  // sol-r1c, zzemu_def probe 2026-11-08 (D1). A config declaring `default: null` is served (sol-r1
  // refused it as an invalid default), the default lands as a stored null, and an explicit null is
  // accepted. Discriminating: the same {text: null} against default: "x" is 403 NOT_NULL.
  const textType = (text: Document): IdmEmulator => new IdmEmulator(new IdmConfig({ sync: { mappings: [] },
    managed: { objects: [{ name: "gadget", schema: { required: [], properties: { text, note: { type: "string" } } } }] } }), runner);
  it("a create omitting a null-defaulted property stores and returns text: null", async () => {
    const emu = textType({ type: "string", default: null });
    const result = await emu.request({ ...createPath, body: { _id: "d1", note: "a" } });
    expect(result.status).toBe(201);
    expect(result.body).toMatchObject({ text: null, note: "a" });
    expect(Object.hasOwn(result.body as Document, "text")).toBe(true);
    expect(emu.emulationFailures).toEqual([]);
    const read = await emu.request({ method: "GET", path: "managed/gadget/d1" });
    expect((read.body as Document).text).toBeNull();
  });
  it.each([
    ["default: null", { type: "string", default: null }, 201],
    ["default: \"x\"", { type: "string", default: "x" }, 403],
  ] as const)("{text: null} against %s", async (_name, text, status) => {
    const result = await textType(text).request({ ...createPath, body: { _id: "d2", text: null } });
    expect(result.status).toBe(status);
    if (status === 403) expect((result.body as Document).detail).toEqual({ result: false,
      failedPolicyRequirements: [{ policyRequirements: [{ policyRequirement: "NOT_NULL" }], property: "text" }] });
  });
  // NOT_NULL is not the boolean type's: an object defaulting to {x:1} gets it too (D4).
  it("a null on an object with a non-null default is NOT_NULL", async () => {
    const result = await textType({ type: "object", default: { x: 1 }, properties: { x: { type: "string" } } })
      .request({ ...createPath, body: { _id: "d3", text: null } });
    expect(result.status).toBe(403);
    expect((result.body as Document).detail).toEqual({ result: false,
      failedPolicyRequirements: [{ policyRequirements: [{ policyRequirement: "NOT_NULL" }], property: "text" }] });
  });

  // P2-3, measured 2026-11-08. Discriminating: the same body against required: [] and no required key.
  it("a null on an undefaulted property with required: [] is stored as null (measured)", async () => {
    const emu = make(gadget([]));
    const result = await emu.request({ ...createPath, body: { _id: "n1", text: null } });
    expect(result.status).toBe(201);
    expect(result.provenance).toContainEqual({ kind: "measured", behavior: POLICY_MEASURED.nullAccepted, source: "docs/api/10-managed-objects.md" });
    expect(emu.emulationFailures).toEqual([]);
    const read = await emu.request({ method: "GET", path: "managed/gadget/n1" });
    expect(Object.hasOwn(read.body as Document, "text") && (read.body as Document).text === null).toBe(true);
  });
  it("the same null with no required key is the measured 403 VALID_TYPE body, and nothing is stored", async () => {
    const emu = make(gadget());
    const result = await emu.request({ ...createPath, body: { _id: "n2", text: null } });
    expect(result.status).toBe(403);
    expect(JSON.stringify(result.body)).toBe('{"code":403,"reason":"Forbidden","message":"Policy validation failed",' +
      '"detail":{"result":false,"failedPolicyRequirements":[{"policyRequirements":[{"params":{"invalidType":"null","validTypes":["string"]},' +
      '"policyRequirement":"VALID_TYPE"}],"property":"text"}]}}');
    expect((await emu.request({ method: "GET", path: "managed/gadget/n2" })).status).toBe(404);
  });
  it("a defaulted null with no required key lists NOT_NULL then VALID_TYPE (measured)", async () => {
    const result = await make(gadget()).request({ ...createPath, body: { flag: null } });
    expect(result.status).toBe(403);
    expect((result.body as Document).detail).toEqual({ result: false, failedPolicyRequirements: [
      { policyRequirements: [{ policyRequirement: "NOT_NULL" }], property: "flag" },
      { policyRequirements: [{ params: { invalidType: "null", validTypes: ["boolean"] }, policyRequirement: "VALID_TYPE" }], property: "flag" },
    ] });
  });

  // The IDM-script surface the probe measured: a catchable JavaException whose Java exception is
  // CREST's ForbiddenException with code 403 and the same detail.
  it("a script's openidm.create sees the measured ForbiddenException, getCode and getDetail", async () => {
    const emu = make(gadget());
    const result = await runner.eval({ engine: "idm", source:
      "var out; try { openidm.create('managed/gadget', 'n3', {text: null}); out = 'stored'; } catch (e) { out = [e.name, e.message, " +
      "e.javaException.getCode(), String(e.javaException.getDetail())].join(' | '); } out;" }, { onCall: emu.onCall });
    expect(result.outcome).toBe("ok");
    const [name, message, code, detail] = String(result.value).split(" | ");
    expect(name).toBe("JavaException");
    expect(message).toBe("org.forgerock.json.resource.ForbiddenException: Policy validation failed");
    expect(code).toBe("403");
    expect(detail).toContain("VALID_TYPE");
    expect(detail).toContain("null");
  });

  // P2-1, measured: only the top-level type is enforced.
  it("a top-level mismatch is the measured 403 VALID_TYPE and does not write", async () => {
    const emu = make(gadget([]));
    const result = await emu.request({ ...createPath, body: { _id: "g2", text: 5 } });
    expect(result.status).toBe(403);
    expect((result.body as Document).detail).toEqual({ result: false, failedPolicyRequirements: [
      { policyRequirements: [{ params: { invalidType: "number", validTypes: ["string"] }, policyRequirement: "VALID_TYPE" }], property: "text" }] });
    expect(result.provenance).toContainEqual({ kind: "measured", behavior: POLICY_MEASURED.topLevelType, source: "docs/api/10-managed-objects.md" });
    expect((await emu.request({ method: "GET", path: "managed/gadget/g2" })).status).toBe(404);
  });
  // Sol's two rows: before this round both were 201 with the false "array-item rules were checked".
  it.each([
    ["a number in a string[] nested in an object", { box: { tags: [2] } }],
    ["a string where items.properties.inner is a number", { rows: [{ inner: "bad" }] }],
  ] as const)("%s is stored as written (measured)", async (_name, body) => {
    const emu = make(gadget([]));
    const result = await emu.request({ ...createPath, body: { _id: "n4", ...(body as unknown as Document) } });
    expect(result.status).toBe(201);
    expect(emu.emulationFailures).toEqual([]);
    expect(result.provenance).toContainEqual({ kind: "measured", behavior: POLICY_MEASURED.nestedUnchecked, source: "docs/api/10-managed-objects.md" });
    expect(JSON.stringify(result.provenance)).not.toContain("were checked");
    expect((await emu.request({ method: "GET", path: "managed/gadget/n4" })).body).toMatchObject(body);
  });
  it("a mismatch over an existing record is the harness fault idm-policy/valid-type", async () => {
    expectHarnessFault("idm-policy/valid-type");
    const emu = make(gadget([]));
    await emu.request({ ...createPath, body: { _id: "g5", text: "a" } });
    const result = await emu.request({ method: "PUT", path: "managed/gadget/g5", body: { text: 5 } });
    expect(result.status).toBe(501);
    expect(JSON.stringify(result.body)).toContain("idm-policy/valid-type");
    expect((await emu.request({ method: "GET", path: "managed/gadget/g5" })).body).toMatchObject({ text: "a" });
  });

  // sol-r2 P2-2 (zzemu_nd N3): IDM types every JSON number "number", so an integer refuses 3.
  it("3 on an integer is the measured 403 VALID_TYPE number, and 3 on a number is stored", async () => {
    const emu = make(widget([]));
    const widgetPath = { method: "POST", path: "managed/widget", query: { _action: "create" } } as const;
    const refused = await emu.request({ ...widgetPath, body: { _id: "w1", i: 3 } });
    expect(refused.status).toBe(403);
    expect((refused.body as { detail: unknown }).detail).toEqual({ result: false, failedPolicyRequirements: [vt("i", "number", "integer")] });
    expect((await emu.request({ method: "GET", path: "managed/widget/w1" })).status).toBe(404);
    const stored = await emu.request({ ...widgetPath, body: { _id: "w2", n: 3 } });
    expect(stored.status).toBe(201);
  });

  // zzemu_nd N5/N8: an array's null default, and an explicit null sent to it, store [].
  it.each([
    ["omitted", { _id: "a1" }],
    ["sent as null", { _id: "a1", list: null }],
  ])("an array with a null default stores [] when %s", async (_name, body) => {
    const emu = make(widget([]));
    const result = await emu.request({ method: "POST", path: "managed/widget", query: { _action: "create" }, body });
    expect(result.status).toBe(201);
    expect((await emu.request({ method: "GET", path: "managed/widget/a1" })).body).toMatchObject({ list: [] });
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

  // astra-r1 P2-3: an operation with no `value` key is the same null, refused at the same point
  // (before lookup and read hooks), as the AM lane refuses it. Discriminating: at d7212ed every
  // "omitted" row answered 404 on the absent record and ran the read hooks on the existing one.
  // The "undefined" rows send a host object whose `value` key is present and undefined.
  const VALUES = [["omitted", {}], ["null", { value: null }], ["undefined", { value: undefined }]] as const;
  const RECORDS = [["an absent record", "absent"], ["an existing record with read hooks", "one"]] as const;
  const crossed = VALUES.flatMap(([value, extra]) => RECORDS.flatMap(([record, id]) =>
    (["replace", "add"] as const).map((operation) => [operation, value, record, id, extra] as const)));
  it.each(crossed)("REST: %s with value %s on %s is refused before lookup", async (operation, _value, _record, id, extra) => {
    const emu = traced(runner);
    await create(emu);
    const before = (await trace(emu)).length;
    if (operation === "add") expectHarnessFault("idm-patch/add-null");
    const result = await emu.request({ method: "PATCH", path: `managed/widget/${id}`, body: [{ operation, field: "/text", ...extra } as Document] });
    if (operation === "replace") {
      expect(result.status).toBe(400);
      expect(String((result.body as Document).message)).toContain("not a valid JSON patch: replace /text with null");
    } else {
      expect(result.status).toBe(501);
      expect(JSON.stringify(result.body)).toContain("idm-patch/add-null");
    }
    expect((await trace(emu)).length).toBe(before);
    expect((await emu.request({ method: "GET", path: "managed/widget/one" })).body).toMatchObject({ text: "a" });
  });
  // The script path: an IDM script's openidm.patch, through the emulator's host call.
  const SCRIPT_VALUES = [["omitted", ""], ["null", ", value: null"], ["undefined", ", value: undefined"]] as const;
  const scripted = SCRIPT_VALUES.flatMap(([value, literal]) => RECORDS.flatMap(([record, id]) =>
    (["replace", "add"] as const).map((operation) => [operation, value, record, id, literal] as const)));
  it.each(scripted)("script: %s with value %s on %s is refused before lookup", async (operation, _value, _record, id, literal) => {
    const emu = traced(runner);
    await create(emu);
    const before = (await trace(emu)).length;
    if (operation === "add") expectHarnessFault("idm-patch/add-null");
    const result = await runner.eval({ engine: "idm", source:
      `var out; try { openidm.patch('managed/widget/${id}', null, [{operation: '${operation}', field: '/text'${literal}}]); out = 'patched'; } ` +
      "catch (e) { out = String(e); } out;" }, { onCall: emu.onCall });
    if (operation === "replace") {
      expect(result.outcome).toBe("ok");
      expect(String(result.value)).toContain("not a valid JSON patch: replace /text with null");
    } else {
      expect(result.outcome).toBe("runtime_error");
      expect(JSON.stringify(result.emulation)).toContain("idm-patch/add-null");
    }
    expect((await trace(emu)).length).toBe(before);
  });

  // Fix round 2, P3-3 end to end: removing an optional undefaulted property commits.
  // Measured 2026-11-08 under both required shapes. The tenant then reads the key as absent; this
  // emulator's generic materialization still reads it back as null (open: see the gaps doc).
  it.each([["required: []", []], ["no required key", undefined]] as const)("remove of an undefaulted optional property commits (measured), %s", async (_name, required) => {
    const emu = await seeded(required as string[] | undefined);
    const result = await emu.request(patch([{ operation: "remove", field: "/text" }]));
    expect(result.status).toBe(200);
    expect(result.provenance).toContainEqual({ kind: "measured", behavior: POLICY_MEASURED.optionalRemove, source: "docs/api/10-managed-objects.md" });
    expect(emu.emulationFailures).toEqual([]);
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
