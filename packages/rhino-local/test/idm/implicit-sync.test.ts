/**
 * Implicit sync (E15 round 1, P2-4). On AIC a committed managed write on a sync mapping's source
 * runs the mapping (docs/api/16-sync-mappings.md, measured 2026-06-19). The emulator does not
 * model sync, so such a write is the harness fault `idm-implicit-sync/<mapping>`, and nothing is
 * stored. Mapping names here are invented.
 */
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { expectHarnessFault } from "../../src/harness/emulation-guard.ts";
import { IdmConfig, IdmEmulator, type ConfigInput, type ManagedType } from "../../src/idm/index.ts";
import type { RestRequest } from "../../src/idm/types.ts";
import { RhinoRunner } from "../../src/runner.ts";
import { js, send } from "./fixtures.ts";

let runner: RhinoRunner;
beforeAll(async () => { runner = await RhinoRunner.spawn(); }, 60_000);
afterAll(async () => { await runner?.close(); }, 30_000);

const widget: ManagedType = { name: "widget", schema: { properties: { text: { type: "string" }, seen: { type: "string" } } } };
const gadget: ManagedType = { name: "gadget", schema: { properties: { text: { type: "string" } } } };
const mapping = (name: string, source: string) => ({ name, source, target: "managed/gadget", properties: [] });

function emulator(sync?: ConfigInput["sync"]): IdmEmulator {
  return new IdmEmulator(new IdmConfig({ managed: { objects: [widget, gadget] }, ...(sync === undefined ? {} : { sync }) }), runner);
}

/**
 * An emulator over `sync` holding one stored widget w1. The record is written by an emulator
 * without sync and copied in, as `fork()` copies records: a create on a mapped type faults.
 */
async function seeded(sync: ConfigInput["sync"]): Promise<IdmEmulator> {
  const plain = emulator();
  await send(plain, { method: "PUT", path: "managed/widget/w1", headers: { "If-None-Match": "*" }, body: { text: "a" } }, 201);
  const mapped = emulator(sync);
  const records = (emu: IdmEmulator) => (emu as unknown as { records: Map<string, unknown> }).records;
  for (const [key, value] of records(plain)) records(mapped).set(key, structuredClone(value));
  return mapped;
}

const create: RestRequest = { method: "PUT", path: "managed/widget/w1", headers: { "If-None-Match": "*" }, body: { text: "a" } };
const read: RestRequest = { method: "GET", path: "managed/widget/w1" };

describe("a write on a mapping's source is the implicit-sync fault", () => {
  const sync = { mappings: [mapping("widgetToGadget", "managed/widget")] };

  it("create faults, names the mapping, and stores nothing", async () => {
    expectHarnessFault("idm-implicit-sync/widgetToGadget");
    const emu = emulator(sync);
    const result = await emu.request(create);
    expect(result.status).toBe(501);
    expect(result.emulation?.faults?.map((fault) => fault.id)).toEqual(["idm-implicit-sync/widgetToGadget"]);
    expect(result.emulation?.faults?.[0]?.message).toContain("managed/widget is the source of sync mapping widgetToGadget");
    expect((await emu.request(read)).status).toBe(404);
  });

  it.each([
    ["PUT with If-Match", { method: "PUT", path: "managed/widget/w1", headers: { "If-Match": "*" }, body: { text: "b" } }],
    ["PATCH that changes a field", { method: "PATCH", path: "managed/widget/w1", body: [{ operation: "replace", field: "/text", value: "b" }] }],
    ["DELETE", { method: "DELETE", path: "managed/widget/w1" }],
  ] as Array<[string, RestRequest]>)("%s faults and leaves the stored record as it was", async (_name, request) => {
    expectHarnessFault("idm-implicit-sync/widgetToGadget");
    const emu = await seeded(sync);
    const result = await emu.request(request);
    expect(result.status).toBe(501);
    expect(result.emulation?.faults?.map((fault) => fault.id)).toEqual(["idm-implicit-sync/widgetToGadget"]);
    expect(await send(emu, read)).toMatchObject({ text: "a", _rev: "0" });
  });

  it("a write from a script that catches it still fails the request", async () => {
    expectHarnessFault("idm-implicit-sync/widgetToGadget");
    const emu = new IdmEmulator(new IdmConfig({ managed: { objects: [widget, gadget] }, sync,
      endpoints: { probe: js("var r; try { openidm.create('managed/widget', 'w2', { text: 'x' }); r = 'created'; } catch (e) { r = 'caught'; } ({ r: r });") } }), runner);
    const result = await emu.request({ method: "GET", path: "endpoint/probe" });
    expect(result.status).toBe(501);
    expect(result.emulation?.faults?.map((fault) => fault.id)).toEqual(["idm-implicit-sync/widgetToGadget"]);
  });

  it("several mappings on one source are all named; the id is the first", async () => {
    expectHarnessFault("idm-implicit-sync/first");
    const result = await emulator({ mappings: [mapping("first", "managed/widget"), mapping("second", "/managed/widget/")] }).request(create);
    expect(result.emulation?.faults?.map((fault) => fault.id)).toEqual(["idm-implicit-sync/first"]);
    expect(result.emulation?.faults?.[0]?.message).toContain("sync mappings first, second");
  });
});

describe("writes that sync nothing", () => {
  it("a PATCH that changes nothing writes nothing, so it does not fault", async () => {
    const emu = await seeded({ mappings: [mapping("widgetToGadget", "managed/widget")] });
    await send(emu, { method: "PATCH", path: "managed/widget/w1", body: [{ operation: "replace", field: "/text", value: "a" }] });
  });

  it("a type that is only a mapping's target, or unmapped, writes, with the reason recorded", async () => {
    const emu = emulator({ mappings: [mapping("widgetToGadget", "managed/widget")] });
    const result = await emu.request({ method: "PUT", path: "managed/gadget/g1", headers: { "If-None-Match": "*" }, body: { text: "a" } });
    expect(result.status).toBe(201);
    expect(result.provenance).toContainEqual({ kind: "assumed", behavior: "the supplied config/sync has no mapping with this type as its source, so no implicit sync runs" });
  });

  it("without config/sync a write passes and records the assumption, never silently", async () => {
    const result = await emulator().request(create);
    expect(result.status).toBe(201);
    expect(result.provenance).toContainEqual({ kind: "assumed",
      behavior: "no config/sync supplied: no sync mapping is assumed to have this type as its source, so no implicit sync runs" });
  });
});

describe("sync configuration", () => {
  it("refuses a malformed config/sync", () => {
    const build = (sync: unknown) => () => new IdmConfig({ managed: { objects: [widget] }, sync: sync as NonNullable<ConfigInput["sync"]> });
    expect(build({})).toThrow(/sync.mappings must be an array/);
    expect(build({ mappings: [{ name: "x" }] })).toThrow(/sync.mappings\[0\] needs a nonempty string name and source/);
    expect(build({ mappings: [null] })).toThrow(/sync.mappings\[0\] must be a plain object/);
  });
});
