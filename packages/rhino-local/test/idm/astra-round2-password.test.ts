import { afterAll, beforeAll, expect, it } from "vitest";
import { IdmConfig, IdmEmulator, type Document } from "../../src/idm/index.ts";
import { expectEmulationFailure } from "../../src/harness/emulation-guard.ts";
import { js } from "./fixtures.ts";
import { RhinoRunner } from "../../src/runner.ts";
let runner: RhinoRunner;
beforeAll(async () => { runner = await RhinoRunner.spawn(); }, 60_000);
afterAll(async () => { await runner?.close(); });
const id = "00000000-0000-0000-0000-000000000001";
const path = `managed/alpha_user/${id}`;
function make(name = "alpha_user") {
  return new IdmEmulator(new IdmConfig({ sync: { mappings: [] }, managed: { objects: [{ name, schema: { properties: { password: { type: "string" }, mail: { type: "string" } } } }] } }), runner);
}
const prefix = "{PBKDF2-HMAC-SHA256}";
it.each([
  `openidm.read('${path}')`,
  `openidm.read('${path}',null,['password'])`,
  `openidm.read('${path}',null,['*'])`,
  "openidm.query('managed/alpha_user',{_queryFilter:'true'}).result[0]",
  "openidm.query('managed/alpha_user',{_queryFilter:'true',_fields:'password'}).result[0]",
])("A2 #1 p22/p23 internal password is encoded in %s", async (expression) => {
  const emu = make();
  await emu.request({ method: "PUT", path, body: { password: "submitted-secret", mail: "Mixed@Example.com" } });
  const result = await runner.eval({ engine: "idm", source: `var row=${expression};({password:row.password,has:'password' in row});` }, { onCall: emu.onCall });
  expect(result.outcome).toBe("ok");
  expect(result.value).toMatchObject({ has: true, password: expect.stringMatching(/^\{PBKDF2-HMAC-SHA256\}.+$/) });
  expect((result.value as Document).password).toHaveLength(90);
  expect((result.value as Document).password).not.toBe("submitted-secret");
});
it("A2 #1 HTTP omission differs from internal exposure and retains precise provenance", async () => {
  const emu = make();
  await emu.request({ method: "PUT", path, body: { password: "submitted-secret" } });
  const internal = await emu.hostRequest({ type: "call", id: "probe", seq: 1, method: "read", args: [path] });
  expect((internal.body as Document).password).toEqual(expect.stringContaining(prefix));
  expect(internal.provenance).toContainEqual({ kind: "measured", behavior: "alpha_user internal read/query exposes encoded password prefix", source: "orchestrator p22/p23, 2026-11-02; docs/api/10-managed-objects.md" });
  expect(internal.provenance.some((entry) => entry.kind === "assumed" && entry.behavior.includes("opaque password body"))).toBe(true);
  const http = await emu.request({ method: "GET", path });
  expect(http.status).toBe(200);
  expect(Object.hasOwn(http.body as Document, "password")).toBe(false);
  const query = await emu.request({ method: "GET", path: "managed/alpha_user", query: { _queryFilter: "true" } });
  expect(Object.hasOwn(((query.body as Document).result as Document[])[0]!, "password")).toBe(false);
});
it("A2 #1 stored password representation survives reads, unrelated patch and forks", async () => {
  const emu = make();
  await emu.request({ method: "PUT", path, body: { password: "submitted-secret" } });
  const first = await runner.eval({ engine: "idm", source: `openidm.read('${path}').password` }, { onCall: emu.onCall });
  expect(first.value).toEqual(expect.stringContaining(prefix));
  await emu.request({ method: "PATCH", path, body: [{ operation: "replace", field: "/mail", value: "changed@example.com" }] });
  const next = await runner.eval({ engine: "idm", source: `openidm.read('${path}').password` }, { onCall: emu.fork().onCall });
  expect(next.value).toBe(first.value);
});
it("A2 #1 bravo_user password remains an explicit unmeasured boundary", async () => {
  expectEmulationFailure("bravo_user password semantics");
  const emu = make("bravo_user");
  const result = await runner.eval({ engine: "idm", source: `try {openidm.create('managed/bravo_user','${id}',{password:'secret'})} catch(e) {} 42;` }, { onCall: emu.onCall });
  expect(result.outcome).not.toBe("ok");
  expect(result.emulation?.missing).toContain("bravo_user password semantics");
});

it.each([null, 7, {}])("A2 #1 unmeasured non-string password %j fails missing", async (password) => {
  expectEmulationFailure("alpha_user password without measured string value");
  const result = await runner.eval({ engine: "idm", source: `try {openidm.create('managed/alpha_user','${id}',{password:${JSON.stringify(password)}})} catch(e) {} 42;` }, { onCall: make().onCall });
  expect(result.emulation?.missing).toContain("alpha_user password without measured string value");
  expect(result.outcome).not.toBe("ok");
});
it("A2 #1 unmeasured retrieval-hook password representation cannot expose plaintext", async () => {
  expectEmulationFailure("alpha_user password representation after retrieval hooks");
  const emu = new IdmEmulator(new IdmConfig({ sync: { mappings: [] }, managed: { objects: [{ name: "alpha_user", schema: { properties: { password: { type: "string", onRetrieve: js("'hook-plaintext'") } } } }] } }), runner);
  await emu.request({ method: "PUT", path, body: { password: "submitted-secret" } });
  const result = await runner.eval({ engine: "idm", source: `try {openidm.read('${path}')} catch(e) {} 42;` }, { onCall: emu.onCall });
  expect(result.emulation?.missing).toContain("alpha_user password representation after retrieval hooks");
  expect(result.outcome).not.toBe("ok");
});
