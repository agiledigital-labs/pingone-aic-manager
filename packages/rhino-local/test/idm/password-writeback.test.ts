import { afterAll, beforeAll, expect, it } from "vitest";
import { expectEmulationFailure } from "../../src/harness/emulation-guard.ts";
import { IdmConfig, IdmEmulator } from "../../src/idm/index.ts";
import { RhinoRunner } from "../../src/runner.ts";
let runner: RhinoRunner;
beforeAll(async () => { runner = await RhinoRunner.spawn(); }, 60_000);
afterAll(async () => { await runner?.close(); });
const id = "00000000-0000-0000-0000-000000000001";
const otherId = "00000000-0000-0000-0000-000000000002";
const path = `managed/alpha_user/${id}`;
const prefix = "{PBKDF2-HMAC-SHA256}";
const source = "orchestrator p25, 2026-11-02; docs/api/10-managed-objects.md";
const behavior = "alpha_user encoded password survives update, password PATCH and cross-record create verbatim";
function make() { return new IdmEmulator(new IdmConfig({ sync: { mappings: [] }, managed: { objects: [{ name: "alpha_user", schema: { properties: { password: { type: "string" }, sn: { type: "string" } } } }] } }), runner); }
const setup = `openidm.create('managed/alpha_user','${id}',{password:'submitted-secret',sn:'before'});var first=openidm.read('${path}').password;`;
it.each([
  ["update", `var object=openidm.read('${path}');object.sn='after';openidm.update('${path}',null,object);`, path],
  ["patch", `openidm.patch('${path}',null,[{operation:'replace',field:'password',value:first}]);`, path],
  ["create", `openidm.create('managed/alpha_user','${otherId}',{password:first,sn:'copy'});`, `managed/alpha_user/${otherId}`],
])("p25 %s preserves an already encoded password and reports measured provenance", async (_method, write, readPath) => {
  const emu = make();
  const result = await runner.eval({ engine: "idm", source: `${setup}${write}({same:openidm.read('${readPath}').password===first,length:first.length});` }, { onCall: emu.onCall });
  expect(result).toMatchObject({ outcome: "ok", value: { same: true, length: 90 } });
  expect(result.provenance).toContainEqual({ kind: "measured", behavior, source });
  expect(result.assumed).toEqual(expect.arrayContaining([expect.stringContaining("opaque password body")]));
  expect(result.assumed?.some((entry) => entry.includes("unchanged-token retention"))).toBe(false);
});
for (const [password, cause] of [
  [prefix, "alpha_user password prefixed body is unmeasured"],
  [`${prefix}malformed`, "alpha_user password prefixed body is unmeasured"],
  [`${prefix}${"a".repeat(70)}`, "alpha_user password prefixed body is unmeasured"],
  ["{SHA256}foreign", "alpha_user password scheme is unmeasured"],
] as const) {
  it.each(["create", "update", "patch"])(`unmeasured password ${password} is fatal on %s, even when caught`, async (method) => {
    expectEmulationFailure(cause);
    const emu = make();
    const value = JSON.stringify(password);
    const write = method === "create" ? `openidm.create('managed/alpha_user','${otherId}',{password:${value}})` : method === "update" ? `openidm.update('${path}',null,{password:${value},sn:'changed'})` : `openidm.patch('${path}',null,[{operation:'replace',field:'password',value:${value}}])`;
    const result = await runner.eval({ engine: "idm", source: `${setup}try {${write}} catch(e) {} 42;` }, { onCall: emu.onCall });
    expect(result.outcome).not.toBe("ok");
    expect(result.emulation?.missing).toContain(cause);
    const read = await runner.eval({ engine: "idm", source: `({hash:openidm.read('${path}').password,absent:openidm.read('managed/alpha_user/${otherId}')===null})` }, { onCall: emu.onCall });
    expect(read).toMatchObject({ outcome: "ok", value: { hash: expect.stringMatching(/^\{PBKDF2-HMAC-SHA256\}.+$/), absent: true } });
    expect((await emu.request({ method: "GET", path })).body).toMatchObject({ _rev: "0", sn: "before" });
  });
}
it("p25 opaque token recognition survives forks without depending on the destination's old password", async () => {
  const emu = make();
  const created = await runner.eval({ engine: "idm", source: `${setup}first;` }, { onCall: emu.onCall });
  expect(created.outcome).toBe("ok");
  const child = emu.fork();
  const result = await runner.eval({ engine: "idm", source: `openidm.create('managed/alpha_user','${otherId}',{password:${JSON.stringify(created.value)}});openidm.read('managed/alpha_user/${otherId}').password;` }, { onCall: child.onCall });
  expect(result).toMatchObject({ outcome: "ok", value: created.value });
  expect(result.provenance).toContainEqual({ kind: "measured", behavior, source });
});
it("p25 matches remains fatal missing despite observed false results", async () => {
  expectEmulationFailure("openidm.matches");
  const result = await runner.eval({ engine: "idm", source: `${setup}try {openidm.matches('submitted-secret',first)} catch(e) {} 42;` }, { onCall: make().onCall });
  expect(result.outcome).not.toBe("ok");
  expect(result.emulation?.missing).toContain("openidm.matches");
});
