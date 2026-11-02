import { afterAll, beforeAll, expect, it } from "vitest";
import { expectEmulationFailure } from "../../src/harness/emulation-guard.ts";
import { IdmConfig, IdmEmulator, type Document } from "../../src/idm/index.ts";
import { RhinoRunner } from "../../src/runner.ts";
import { emulator, simple } from "./fixtures.ts";
let runner: RhinoRunner;
beforeAll(async () => { runner = await RhinoRunner.spawn(); }, 60_000);
afterAll(async () => { await runner?.close(); });
const routes = ["system/hr/account/123", "repo/internal/user/123", "config/sync", "config/access", "internal/user/123", "unrouted/path", "managed/notConfigured/123", "endpoint/notConfigured"];
it.each(routes)("Astra #1 route knowledge determines the outcome for read %s", async (path) => {
  const knownAbsent = path.startsWith("managed/notConfigured") || path === "endpoint/notConfigured";
  if (!knownAbsent) expectEmulationFailure(`resource route: ${path}`);
  const result = await runner.eval({ engine: "idm", source: `(function(){try { return openidm.read(${JSON.stringify(path)}); } catch(e) {} finally { 42; }})()` }, { onCall: new IdmEmulator(new IdmConfig({ managed: { objects: [simple] }, endpoints: {}, complete: { managed: true, endpoints: true } }), runner).onCall });
  if (knownAbsent) expect(result).toMatchObject({ outcome: "ok", value: null });
  else {
    expect(result.outcome).not.toBe("ok");
    expect(result.emulation?.missing).toContain(`resource route: ${path}`);
  }
});
it.each(["GET", "POST", "PUT", "PATCH", "DELETE"] as const)("Astra #1 unsupported REST %s route names its cause", async (method) => {
  expectEmulationFailure("resource route: other/a");
  expect(await emulator(runner).request({ method, path: "other/a", body: {} })).toMatchObject({ status: 501, emulation: { kind: "missing", missing: ["resource route: other/a"] } });
});
it("Astra #1 absence within an implemented collection still reads null", async () => {
  const result = await runner.eval({ engine: "idm", source: "openidm.read('managed/widget/absent')" }, { onCall: emulator(runner).onCall });
  expect(result).toMatchObject({ outcome: "ok", value: null });
});
it.each(["patch", "triggerSyncCheck", "updateLastSync"])("Astra #4 collection action %s is fatal", async (action) => {
  expectEmulationFailure(`managed collection action: ${action}`);
  const result = await runner.eval({ engine: "idm", source: `try { openidm.action('managed/widget',${JSON.stringify(action)},{}); } catch(e) {} 42;` }, { onCall: emulator(runner).onCall });
  expect(result.outcome).not.toBe("ok");
  expect(result.emulation?.missing).toContain(`managed collection action: ${action}`);
});
it("Astra #4 unknown action remains a catchable measured 400", async () => {
  const result = await runner.eval({ engine: "idm", source: "try { openidm.action('managed/widget','unknown',{}); } catch(e) { e.javaException.getCode(); }" }, { onCall: emulator(runner).onCall });
  expect(result).toMatchObject({ outcome: "ok", value: 400 });
});
it.each(["alpha_user", "bravo_user"])("Astra #2 %s rejects readable ids at the repository boundary", async (name) => {
  const emu = new IdmEmulator(new IdmConfig({ managed: { objects: [{ name, schema: { properties: { userName: { type: "string" } } } }] } }), runner);
  const result = await runner.eval({ engine: "idm", source: `try { openidm.create('managed/${name}','readable',{userName:'probe'}); } catch(e) { ({code:e.javaException.getCode(),message:e.message}); }` }, { onCall: emu.onCall });
  expect(result.outcome).toBe("ok");
  expect(result.value).toMatchObject({ code: 400, message: expect.stringContaining('org.forgerock.json.resource.BadRequestException: Invalid Attribute Syntax: Entry "fr-idm-uuid=readable,ou=user,') });
  expect((result.value as Document).message).toContain('contains a value "readable"');
  const absent = await runner.eval({ engine: "idm", source: `openidm.read('managed/${name}/readable')` }, { onCall: emu.onCall });
  expect(absent).toMatchObject({ outcome: "ok", value: null });
  const valid = await emu.request({ method: "PUT", path: `managed/${name}/00000000-0000-0000-0000-000000000001`, body: {} });
  expect(valid.status).toBe(201);
});
it.each(["alpha_role", "custom_user"])("Astra #2 %s retains caller-chosen ids", async (name) => {
  const emu = new IdmEmulator(new IdmConfig({ managed: { objects: [{ name, schema: { properties: {} } }] } }), runner);
  expect((await emu.request({ method: "PUT", path: `managed/${name}/readable`, body: {} })).status).toBe(201);
});
