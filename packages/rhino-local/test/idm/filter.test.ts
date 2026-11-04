import { expectEmulationFailure } from "../../src/harness/emulation-guard.ts";
/** CREST grammar evaluated by querying the emulator, including a real script caller. */
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { RhinoRunner } from "../../src/runner.ts";
import { create, emulator, send } from "./fixtures.ts";
import { type Document, IdmConfig, IdmEmulator } from "../../src/idm/index.ts";

let runner: RhinoRunner;
let seeded: IdmEmulator;
beforeAll(async () => {
  runner = await RhinoRunner.spawn();
  seeded = emulator(runner);
  await create(seeded, { text: "alpha", count: 1, flag: true, nested: { "a/b": "match", name: "Zulu" } }, "a");
  await create(seeded, { text: "beta", count: 2, flag: false, nested: { name: "Able" } }, "b");
  await create(seeded, { text: "alpine", count: 3, flag: true }, "c");
}, 60_000);
afterAll(async () => { await runner?.close(); }, 30_000);

describe("queryFilter grammar", () => {
  it.each([
    ["true", ["a", "b", "c"]], ["false", []],
    ['text eq "alpha"', ["a"]], ['count eq "2"', ["b"]],
    ['text co "ph"', ["a"]], ['text sw "al"', ["a", "c"]],
    ["count gt 2", ["c"]], ["count ge 2", ["b", "c"]],
    ["count lt 2", ["a"]], ["count le 2", ["a", "b"]],
    ["flag eq true", ["a", "c"]], ["flag eq false", ["b"]],
    ["text pr", ["a", "b", "c"]], ["missing pr", []], ["other pr", []],
    ["!(text pr)", []], ["!(missing pr)", ["a", "b", "c"]],
    ['text eq "alpha" or text eq "beta" and count eq 3', ["a"]],
    ['(text eq "alpha" or text eq "beta") and count eq 3', []],
    ['!(count lt 2 or count gt 2)', ["b"]],
    ['/nested/name eq "Able"', ["b"]], ['/nested/a~1b eq "match"', ["a"]],
    ['count gt 1.5', ["b", "c"]], ['text co ""', ["a", "b", "c"]],
  ])("%s", async (filter, ids) => {
    const emu = seeded.fork();
    // Going through an endpoint also verifies query parameters across the host bridge.
    const response = await runner.eval({ engine: "idm", source: "openidm.query('managed/widget',{_queryFilter:filter,_fields:['_id']});", bindings: { filter: String(filter) } }, { onCall: emu.onCall });
    expect(response.outcome, JSON.stringify(response.error)).toBe("ok");
    expect(((response.value as Document).result as Document[]).map((row) => row._id)).toEqual(ids);
  });

  it.each([
    "", "not (text pr)", "text in [1]", "text eq [1]", "text eq null", "text eq", "(",
    "text pr trailing", "count eq 1.2.3", 'text eq "unterminated', "count eq 1 and", "true false",
  ])("unparseable %s has the tenant's 400 message", async (filter) => {
    const body = await send(seeded, { method: "GET", path: "managed/widget", query: { _queryFilter: filter } }, 400);
    expect(body).toEqual({ code: 400, reason: "Bad Request", message: "A value could not be parsed as a valid query filter" });
  });

  it("rejects ne with the measured operator-specific diagnostic", async () => {
    const body = await send(seeded, { method: "GET", path: "managed/widget", query: { _queryFilter: 'text ne "x"' } }, 400);
    expect(body.message).toBe("unrecognized or unsupported filter operator 'ne'");
  });

  it("no selector and multiple selectors have the measured 400", async () => {
    for (const query of [{}, { _queryFilter: "true", _queryId: "all" }]) {
      const body = await send(seeded, { method: "GET", path: "managed/widget", query }, 400);
      expect(body.message).toBe("Only one of [_queryId, _queryExpression, _queryFilter] is supported; multiple detected");
    }
  });

  it("query expressions return the measured DS refusal; named query ids fail as missing", async () => {
    expectEmulationFailure("named _queryId: query-all-ids");
    const expression = await send(seeded, { method: "GET", path: "managed/widget", query: { _queryExpression: "select *" } }, 405);
    expect(expression.message).toBe("Query Expressions are not supported when using DS as the repo");
    const result = await seeded.request({ method: "GET", path: "managed/widget", query: { _queryId: "query-all-ids" } });
    expect(result.status).toBe(501);
    expect(result.provenance).toContainEqual({ kind: "missing", behavior: "named _queryId: query-all-ids" });
  });

  it("escaped double quotes parse but do not match, as measured against DS", async () => {
    const emu = seeded.fork();
    await create(emu, { text: 'double"quote' }, "quoted");
    const body = await send(emu, { method: "GET", path: "managed/widget", query: { _queryFilter: 'text eq "double\\"quote"' } });
    expect(body.resultCount).toBe(0);
  });

  // docs/api/10 (2026-11-04): eq on alpha_user mail and userName ignores letter case. Nothing
  // else was probed, so a match that turns on case alone elsewhere is missing, not guessed.
  describe("letter case", () => {
    const user = "00000000-0000-0000-0000-0000000000a1";
    const CASE = { kind: "measured", behavior: "eq on alpha_user mail and userName ignores letter case", source: "docs/api/10-managed-objects.md (eq on mail and userName is case-insensitive, 2026-11-04)" };
    const users = async () => {
      const props = { userName: { type: "string" }, mail: { type: "string" }, givenName: { type: "string" } };
      const emu = new IdmEmulator(new IdmConfig({ managed: { objects: [{ name: "alpha_user", schema: { properties: props } }, { name: "bravo_user", schema: { properties: props } }] } }), runner);
      for (const type of ["alpha_user", "bravo_user"]) await send(emu, { method: "PUT", path: `managed/${type}/${user}`, headers: { "If-None-Match": "*" }, body: { userName: "ua", mail: "ma@example.com", givenName: "alice" } }, 201);
      return emu;
    };
    it.each([
      ['mail eq "mA@eXaMpLe.CoM"'], ['userName eq "uA"'], ['/mail eq "MA@EXAMPLE.COM"'],
    ])("alpha_user %s matches the lower-case record, as measured", async (filter) => {
      const emu = await users();
      const result = await emu.request({ method: "GET", path: "managed/alpha_user", query: { _queryFilter: filter } });
      expect((result.body as Document).resultCount).toBe(1);
      expect(result.provenance).toContainEqual(CASE);
      // Through a script's openidm.query too.
      const scripted = await runner.eval({ engine: "idm", source: "openidm.query('managed/alpha_user',{_queryFilter:filter}).resultCount;", bindings: { filter } }, { onCall: emu.onCall });
      expect(scripted.value).toBe(1);
    });
    it.each([
      ['mail eq "ma@example.com"', "alpha_user", 1], ['mail eq "mb@example.com"', "alpha_user", 0], ['mail sw "ma@"', "alpha_user", 1],
      ['givenName eq "alice"', "alpha_user", 1], ['mail eq "ma@example.com"', "bravo_user", 1],
    ])("%s on %s needs no case rule", async (filter, type, count) => {
      const emu = await users();
      const result = await emu.request({ method: "GET", path: `managed/${type}`, query: { _queryFilter: filter } });
      expect((result.body as Document).resultCount).toBe(count);
      expect(result.provenance).not.toContainEqual(CASE);
    });
    it.each([
      ['mail sw "MA@"', "alpha_user", "query sw whose match turns on letter case: alpha_user mail"],
      ['mail co "EXAMPLE"', "alpha_user", "query co whose match turns on letter case: alpha_user mail"],
      ['givenName eq "Alice"', "alpha_user", "query eq whose match turns on letter case: alpha_user givenName"],
      ['mail eq "MA@example.com"', "bravo_user", "query eq whose match turns on letter case: bravo_user mail"],
      ['userName eq "UA"', "bravo_user", "query eq whose match turns on letter case: bravo_user userName"],
    ])("%s on %s is missing: case sensitivity there is unmeasured", async (filter, type, cause) => {
      expectEmulationFailure(cause);
      const result = await (await users()).request({ method: "GET", path: `managed/${type}`, query: { _queryFilter: filter } });
      expect(result.status).toBe(501);
      expect(result.provenance).toContainEqual({ kind: "missing", behavior: cause });
    });
    it("a non-ASCII case difference on a measured field is missing", async () => {
      const cause = "query eq whose match turns on letter case: alpha_user userName";
      expectEmulationFailure(cause);
      const emu = await users();
      await send(emu, { method: "PUT", path: "managed/alpha_user/00000000-0000-0000-0000-0000000000a2", headers: { "If-None-Match": "*" }, body: { userName: "émile" } }, 201);
      const result = await emu.request({ method: "GET", path: "managed/alpha_user", query: { _queryFilter: 'userName eq "Émile"' } });
      expect(result.status).toBe(501);
    });
    it("a case-only eq on an unprobed managed type is missing", async () => {
      const cause = "query eq whose match turns on letter case: widget text";
      expectEmulationFailure(cause);
      const result = await seeded.fork().request({ method: "GET", path: "managed/widget", query: { _queryFilter: 'text eq "ALPHA"' } });
      expect(result.status).toBe(501);
    });
  });
});
