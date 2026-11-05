import { afterAll, beforeAll, expect, it } from "vitest";
import { expectEmulationFailure } from "../../src/harness/emulation-guard.ts";
import { IdmConfig, IdmEmulator } from "../../src/idm/index.ts";
import { RhinoRunner } from "../../src/runner.ts";
let runner: RhinoRunner;
beforeAll(async () => { runner = await RhinoRunner.spawn(); }, 60_000);
afterAll(async () => { await runner?.close(); });
for (const complete of [{ managed: true }, { endpoints: true }, { managed: true, endpoints: true }]) {
  it.each([
    ["read", "managed"], ["read", "/openidm/managed/"], ["query", "managed"],
    ["query", "/openidm/managed/"],
    ...["endpoint", "config", "config/endpoint", "system", "repo", "internal"].flatMap((root) =>
      [root, `${root}/`, `/openidm/${root}`, `/openidm/${root}/`].flatMap((path) => [["read", path], ["query", path]])),
    ["read", "managed/"], ["read", "/openidm/managed"],
  ])(`A5 namespace root %s %s stays missing with ${JSON.stringify(complete)}`, async (method, path) => {
    const normalized = path?.replace(/^\/?(?:openidm\/)?/, "").replace(/\/$/, "");
    const cause = `resource route: ${normalized}`;
    expectEmulationFailure(cause);
    const emu = new IdmEmulator(new IdmConfig({ sync: { mappings: [] }, managed: { objects: [] }, endpoints: {}, complete }), runner);
    const result = await runner.eval({ engine: "idm", source: `try {openidm.${method}(${JSON.stringify(path)}${method === "query" ? ",{_queryFilter:'true'}" : ""})} catch(e) {} 42;` }, { onCall: emu.onCall });
    expect(result.outcome).not.toBe("ok");
    expect(result.emulation?.missing).toContain(cause);
    expect(result.assumed ?? []).not.toContain("caller declared endpoints namespace complete");
    expect(result.assumed ?? []).not.toContain("caller declared managed namespace complete");
    const rest = await emu.request({ method: "GET", path: path ?? "" });
    expect(rest).toMatchObject({ status: 501, emulation: { kind: "missing", missing: [cause] } });
  });
}
for (const complete of [{ managed: true }, { endpoints: true }, { managed: true, endpoints: true }]) {
  it.each(["managed", "endpoint", "config", "config/endpoint"])(`A5 create cannot append an id to bypass root refusal: %s ${JSON.stringify(complete)}`, async (root) => {
    const cause = `resource route: ${root}`;
    expectEmulationFailure(cause);
    const emu = new IdmEmulator(new IdmConfig({ sync: { mappings: [] }, managed: { objects: [] }, endpoints: { actual: { type: "text/javascript", source: "({created:true});" } }, complete }), runner);
    for (const path of [root, `/openidm/${root}/`]) {
      const result = await runner.eval({ engine: "idm", source: `try {openidm.create(${JSON.stringify(path)},'actual',{})} catch(e) {} 42;` }, { onCall: emu.onCall });
      expect(result.outcome).not.toBe("ok");
      expect(result.emulation?.missing).toContain(cause);
      for (const method of ["POST", "PUT", "PATCH", "DELETE"] as const) {
        expect(await emu.request({ method, path, body: {} })).toMatchObject({ status: 501, emulation: { kind: "missing", missing: [cause] } });
      }
    }
  });
}
