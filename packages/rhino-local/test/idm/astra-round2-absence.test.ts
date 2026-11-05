import { afterAll, beforeAll, expect, it } from "vitest";
import { IdmConfig, IdmEmulator } from "../../src/idm/index.ts";
import { expectEmulationFailure } from "../../src/harness/emulation-guard.ts";
import { RhinoRunner } from "../../src/runner.ts";
import { js } from "./fixtures.ts";
let runner: RhinoRunner;
beforeAll(async () => { runner = await RhinoRunner.spawn(); }, 60_000);
afterAll(async () => { await runner?.close(); });
function make() { return new IdmEmulator(new IdmConfig({ sync: { mappings: [] }, managed: { objects: [{ name: "widget", schema: { properties: {} } }] }, endpoints: { actual: js("({ok:true});") }, complete: { managed: true, endpoints: true } }), runner); }
it.each(["managed/notConfigured/abc", "endpoint/notConfigured", "config/endpoint/notConfigured"])("A2 #5 authoritative namespace returns null for %s", async (path) => {
  const emu = make();
  const result = await runner.eval({ engine: "idm", source: `openidm.read(${JSON.stringify(path)})` }, { onCall: emu.onCall });
  expect(result).toMatchObject({ outcome: "ok", value: null });
  expect((await emu.request({ method: "GET", path })).status).toBe(404);
  expect(emu.emulationFailures).toHaveLength(0);
});
it.each(["managed/notConfigured", "endpoint/notConfigured"])("A2 #5 query of %s throws NotFoundException rather than missing", async (collection) => {
  const result = await runner.eval({ engine: "idm", source: `try {openidm.query('${collection}',{_queryFilter:'true'})} catch(e) {({code:e.javaException.getCode(),message:e.message})}` }, { onCall: make().onCall });
  expect(result).toMatchObject({ outcome: "ok", value: { code: 404, message: `org.forgerock.json.resource.NotFoundException: Resource '${collection}' not found` } });
});
it.each(["config/notGiven", "system/notGiven/account/1", "repo/notGiven/1", "other/path"])("A2 #5 non-authoritative %s stays fatal", async (path) => {
  expectEmulationFailure(`resource route: ${path}`);
  const result = await runner.eval({ engine: "idm", source: `try {openidm.read('${path}')} catch(e) {} 42;` }, { onCall: make().onCall });
  expect(result.outcome).not.toBe("ok");
  expect(result.emulation?.missing).toContain(`resource route: ${path}`);
});
it("A2 #5 known absence is consistent in forks without hiding implemented endpoints", async () => {
  const child = make().fork();
  const result = await runner.eval({ engine: "idm", source: "({absent:openidm.read('managed/notConfigured/abc'),actual:openidm.read('endpoint/actual').ok})" }, { onCall: child.onCall });
  expect(result).toMatchObject({ outcome: "ok", value: { absent: null, actual: true } });
});
