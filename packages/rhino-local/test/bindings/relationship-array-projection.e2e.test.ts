/**
 * `fields` selectors on a has-many relationship, on the real JVM, compared
 * with the live capture (array-relationship-fields probe, sandbox, 2026-11-08;
 * docs/api/12). The probe creates its records, which the emulator would store
 * as bare references, so this drives the same reads against a seeded,
 * pre-expanded user and compares each element's key set with the capture's.
 * AIC's element order varied between reads; the emulator keeps seeded order.
 */
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { mockPreamble, withHarvest } from "../../src/bindings/preamble.ts";
import { parseHarvest } from "../../src/bindings/harvest.ts";
import type { Given, JsonObject } from "../../src/case/types.ts";
import { expectHarnessFault } from "../../src/harness/emulation-guard.ts";
import { RhinoRunner } from "../../src/runner.ts";

const role = (id: string): JsonObject => ({
  _id: id,
  _rev: `${id}-rev`,
  _ref: `managed/alpha_role/${id}`,
  _refResourceCollection: "managed/alpha_role",
  _refResourceId: id,
  _refResourceRev: `${id}-rev`,
  _refProperties: { _id: `${id}-edge`, _rev: `${id}-edge-rev` },
  name: id,
  description: "zzemu probe",
});
const user = { _id: "u1", _rev: "1", userName: "zzemu-user", sn: "Emu", roles: [role("role-b"), role("role-a")] };
const given = (u: JsonObject = user): Given => ({ managed: { "managed/alpha_user": [u] } });

const BARE = "_ref,_refProperties,_refResourceCollection,_refResourceId";
const SYSTEM = "_id,_ref,_refProperties,_refResourceCollection,_refResourceId,_refResourceRev,_rev";
const FULL = `${SYSTEM},description,name`;
// Per selector: the user's sorted keys and each role element's sorted keys, as captured.
const captured: [string[], string, string][] = [
  [["roles"], "_id,_rev,roles", BARE],
  [["roles/*"], "_id,_rev,roles", FULL],
  [["roles/_id"], "_id,_rev,roles", SYSTEM],
  [["roles/name"], "_id,_rev,roles", SYSTEM],
  [["roles/_ref"], "_id,_rev,roles", SYSTEM],
  [["roles/_refProperties"], "_id,_rev,roles", SYSTEM],
  [["roles/0"], "_id,_rev,roles", SYSTEM],
  [["roles/*/name"], "_id,_rev,roles", `${SYSTEM},name`],
  [["roles/*/name/x"], "_id,_rev,roles", `${SYSTEM},name`],
  [["roles/*/_id"], "_id,_rev,roles", SYSTEM],
  [["roles/*/_refProperties"], "_id,_rev,roles", SYSTEM],
  [["roles/*", "userName"], "_id,_rev,roles,userName", FULL],
  [["roles/*", "roles/_id"], "_id,_rev,roles", FULL],
  [["roles", "roles/*"], "_id,_rev,roles", FULL],
  // The capture's top level is the whole alpha_user schema; the seed's is these four keys.
  [["*", "roles/*"], "_id,_rev,roles,sn,userName", FULL],
  [["*", "roles/name"], "_id,_rev,roles,sn,userName", SYSTEM],
];

const read = (fields: string[]) =>
  `var rec = openidm.read("managed/alpha_user/u1", null, ${JSON.stringify(fields)});
   nodeState.putShared("r", JSON.stringify(rec));`;

describe("has-many relationship projection matches the live capture", () => {
  let runner: RhinoRunner;
  beforeAll(async () => { runner = await RhinoRunner.spawn(); }, 60_000);
  afterAll(async () => { await runner?.close(); }, 30_000);
  const run = (script: string, g: Given = given()) =>
    runner.eval({ preamble: mockPreamble(g), source: withHarvest(script), sourceName: "probe.js" } as never);
  const observe = async (script: string, g?: Given) => {
    const response = await run(script, g);
    expect(response.outcome, JSON.stringify(response.error)).toBe("ok");
    return JSON.parse(parseHarvest(response.value as string).sharedState.final.r as string) as JsonObject;
  };

  it.each(captured)("%j", async (fields, keys, elementKeys) => {
    const rec = await observe(read(fields));
    expect(Object.keys(rec).sort().join(",")).toBe(keys);
    const roles = rec.roles as JsonObject[];
    expect(roles.map((r) => r._refResourceId)).toEqual(["role-b", "role-a"]);
    for (const [i, r] of roles.entries()) {
      expect(Object.keys(r).sort().join(",")).toBe(elementKeys);
      // Every value is the seed's own: nothing is invented, only selected.
      const seeded = user.roles[i] as JsonObject;
      for (const k of Object.keys(r)) expect(r[k]).toEqual(seeded[k]);
    }
  });

  it("an empty relationship reads as [] under every selector, as captured", async () => {
    for (const fields of [["roles"], ["roles/*"], ["roles/*/name"], ["roles/_id"], ["*", "roles/*"]]) {
      expect((await observe(read(fields), given({ ...user, roles: [] }))).roles).toEqual([]);
    }
  });

  // The client's repro: elements seeded as `{_id, _ref}` only.
  const bare = { _id: "one", links: [{ _id: "two", _ref: "managed/alpha_user/two" }] };
  it("a bare reference derives its envelope from _ref for a selector that does not expand", async () => {
    const rec = await observe(read(["links/_id"]).replace("u1", "one"), given(bare));
    expect(rec).toEqual({
      _id: "one",
      links: [{ _id: "two", _ref: "managed/alpha_user/two", _refResourceCollection: "managed/alpha_user", _refResourceId: "two" }],
    });
  });

  it("expanding a bare reference faults, naming the element, the selector and the call site", async () => {
    expectHarnessFault("openidm-fields-unresolved-ref");
    const response = await run(`\n${read(["links/*"]).replace("u1", "one")}`, given(bare));
    expect(response.outcome).not.toBe("ok");
    const faults = (response as { emulation?: { faults?: { id: string; message: string }[] } }).emulation?.faults ?? [];
    expect(faults.map((f) => f.message)).toEqual([
      'rhino-local: openidm.read managed/alpha_user/one fields "links/*": element 0 of links (managed/alpha_user/two) is a bare reference, ' +
        "and reference expansion is unmeasured; seed that element pre-expanded with the target's fields alongside its _ref " +
        "(called from probe.js:2)",
    ]);
  });

  it("a path below an object field of an element faults rather than guessing", async () => {
    expectHarnessFault("openidm-fields-relationship-deep-path");
    const nested = { ...user, roles: [{ ...role("role-a"), box: { x: 1 } }] };
    const response = await run(read(["roles/*/box/x"]), given(nested));
    expect(response.outcome).not.toBe("ok");
    expect(JSON.stringify(response)).toContain("openidm-fields-relationship-deep-path");
  });

  const deepCases: Array<[string[], string]> = [
    [["roles/*/_refProperties/_id"], "a path below the reference envelope key _refProperties is unmeasured"],
    [["roles/*", "roles/*/_refProperties/_id"], "a path below the reference envelope key _refProperties is unmeasured"],
    [["roles/*/_refProperties/_id", "roles/*"], "a path below the reference envelope key _refProperties is unmeasured"],
    [["roles/*/_id/x"], "a path below the reference envelope key _id is unmeasured"],
    [["roles/_refProperties/_id"], "a path below roles/_refProperties is unmeasured"],
    [["roles/name/x"], "a path below roles/name is unmeasured"],
  ];
  // The selector's shape is what is unmeasured, so an empty relationship
  // refuses too: a fixture with no elements must not bless the selector.
  it.each(deepCases.flatMap(([selectors, message]) => [
    [selectors, message, "populated"] as const,
    [selectors, message, "empty"] as const,
  ]))("a deeper path under an envelope key or a non-* name faults: %j (%s)", async (selectors, message, seed) => {
    expectHarnessFault("openidm-fields-relationship-deep-path");
    const edged = { ...user, roles: seed === "empty" ? [] : [{ ...role("role-a"), _refProperties: { _id: "edge", extra: "unselected" } }] };
    const response = await run(`try { ${read(selectors)} } catch (e) {} finally { 1; }`, given(edged));
    expect(response.outcome).not.toBe("ok");
    const faults = (response as { emulation?: { faults?: { id: string; message: string }[] } }).emulation?.faults ?? [];
    expect(faults.map((f) => f.id)).toEqual(["openidm-fields-relationship-deep-path"]);
    expect(faults[0]?.message).toContain(message);
  });

  it("the measured shallow envelope selectors still succeed", async () => {
    const edged = { ...user, roles: [{ ...role("role-a"), _refProperties: { _id: "edge", extra: "unselected" } }] };
    for (const selectors of [["roles/*/_refProperties"], ["roles/_refProperties"], ["roles/*/name/x"]]) {
      const response = await run(read(selectors), given(edged));
      expect(response.outcome, JSON.stringify(selectors)).toBe("ok");
    }
  });

  it("a child of a non-reference array still faults, saying what the parent is", async () => {
    expectHarnessFault("openidm-fields-non-object-parent");
    const response = await run(read(["tags/x"]), given({ ...user, tags: ["a", "b"] }));
    expect(JSON.stringify(response)).toContain("projecting a child of an array of non-references is unmeasured");
  });
});
