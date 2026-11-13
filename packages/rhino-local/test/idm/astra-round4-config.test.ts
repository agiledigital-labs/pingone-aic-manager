import { afterAll, beforeAll, expect, it } from "vitest";
import { IdmConfig, IdmEmulator, type ConfigInput } from "../../src/idm/index.ts";
import { RhinoRunner } from "../../src/runner.ts";
let runner: RhinoRunner;
beforeAll(async () => { runner = await RhinoRunner.spawn(); }, 60_000);
afterAll(async () => { await runner?.close(); });
for (const namespace of ["endpoints", "managed"] as const) {
  it.each([null, [], Object.setPrototypeOf([], null) as unknown, false, 0, "bad", new Date(0), new Map()].map((value) => [value]))(`A4 #1 rejects malformed ${namespace}: %j`, (value) => {
    const input = { sync: { mappings: [] }, managed: { objects: [] }, [namespace]: value } as unknown as ConfigInput;
    expect(() => new IdmEmulator(new IdmConfig(input), runner)).toThrow(`Invalid IDM configuration: ${namespace} must be a plain object`);
  });
}
it.each([null, {}, false, 0])("A4 #1 rejects malformed managed.objects: %j", (objects) => {
  expect(() => new IdmEmulator(new IdmConfig({ sync: { mappings: [] }, managed: { objects } } as unknown as ConfigInput), runner)).toThrow("Invalid IDM configuration: managed.objects must be an array");
});
it.each([null, [], Object.setPrototypeOf([], null) as unknown, false, 0].map((value) => [value]))("A4 #1 rejects malformed managed type entries: %j", (type) => {
  expect(() => new IdmEmulator(new IdmConfig({ sync: { mappings: [] }, managed: { objects: [type] } } as unknown as ConfigInput), runner)).toThrow("Invalid IDM configuration: managed.objects[0] must be a plain object");
});
it("A4 #1 accepts null-prototype namespace maps without changing implemented routing", async () => {
  const managed = Object.assign(Object.create(null) as object, { objects: [] });
  const endpoints = Object.assign(Object.create(null) as object, { actual: { type: "text/javascript", source: "({answer:42});" } });
  const emu = new IdmEmulator(new IdmConfig({ sync: { mappings: [] }, managed, endpoints } as ConfigInput), runner);
  const result = await runner.eval({ engine: "idm", source: "openidm.read('endpoint/actual')" }, { onCall: emu.onCall });
  expect(result).toMatchObject({ outcome: "ok", value: { answer: 42 } });
});
it.each([null, [], Object.setPrototypeOf([], null) as unknown, false, 0].map((value) => [value]))("A4 #1 rejects malformed root input: %j", (input) => {
  expect(() => new IdmEmulator(new IdmConfig(input as unknown as ConfigInput), runner)).toThrow("Invalid IDM configuration: input must be a plain object");
});
it.each([undefined, 0, ""])("A4 #1 rejects invalid managed type names: %j", (name) => {
  expect(() => new IdmEmulator(new IdmConfig({ sync: { mappings: [] }, managed: { objects: [{ name }] } } as unknown as ConfigInput), runner)).toThrow("Invalid IDM configuration: managed.objects[0].name must be a nonempty string");
});
