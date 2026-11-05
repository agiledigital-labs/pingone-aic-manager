import { expectEmulationFailure } from "../../src/harness/emulation-guard.ts";
/** CREST grammar evaluated by querying the emulator, including a real script caller. */
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { RhinoRunner } from "../../src/runner.ts";
import { create, emulator, send } from "./fixtures.ts";
import { expectHarnessFault } from "../../src/harness/emulation-guard.ts";
import { type CollectionRow, causes, collectionRecords, collectionTable, filterTable, illegalMessage, records, recordsFor, repo } from "../crest-filter-table.ts";
import { escapeError } from "../../src/idm/errors.ts";
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
    ['text eq "alpha"', ["a"]], ['count eq 2', ["b"]],
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
    ['count gt 1', ["b", "c"]], ['text co ""', ["a", "b", "c"]],
  ])("%s", async (filter, ids) => {
    const emu = seeded.fork();
    // Going through an endpoint also verifies query parameters across the host bridge.
    const response = await runner.eval({ engine: "idm", source: "openidm.query('managed/widget',{_queryFilter:filter,_fields:['_id']});", bindings: { filter: String(filter) } }, { onCall: emu.onCall });
    expect(response.outcome, JSON.stringify(response.error)).toBe("ok");
    expect(((response.value as Document).result as Document[]).map((row) => row._id)).toEqual(ids);
  });

  // The shared table (test/crest-filter-table.ts), which the script tester runs too.
  describe("shared table", () => {
    const table = filterTable.map((row) => [JSON.stringify(row.filter), row] as const);
    const fresh = async () => {
      const emu = emulator(runner);
      for (const { _id, ...record } of records) await create(emu, structuredClone(record) as Document, _id);
      return emu;
    };
    it.each(table)("REST %s", async (_name, row) => {
      if (row.rest === "fault") expectHarnessFault("openidm-query-filter");
      if (typeof row.rest === "object" && "missing" in row.rest) expectEmulationFailure(row.rest.missing);
      const result = await (await fresh()).request({ method: "GET", path: "managed/widget", query: { _queryFilter: row.filter as string } });
      const body = result.body as Document;
      if (typeof row.rest === "object" && "missing" in row.rest) {
        expect(result.status, row.why).toBe(501);
        expect(result.provenance, row.why).toContainEqual({ kind: "missing", behavior: row.rest.missing });
      } else if (row.rest === "parse") expect(body, row.why).toEqual({ code: 400, reason: "Bad Request", message: "A value could not be parsed as a valid query filter" });
      else if (row.rest === "ne") expect(body.message, row.why).toBe("unrecognized or unsupported filter operator 'ne'");
      else if (row.rest === "fault") {
        expect(result.status, row.why).toBe(501);
        expect(result.emulation?.faults?.map((fault) => fault.id)).toEqual(["openidm-query-filter"]);
      } else expect(((body.result ?? []) as Document[]).map((r) => r._id), row.why).toEqual(row.rest);
    });
    it.each(table)("script %s", async (_name, row) => {
      if (row.script === "fault") expectHarnessFault("openidm-query-filter");
      if (typeof row.script === "object" && "missing" in row.script) expectEmulationFailure(row.script.missing);
      const source = "try { JSON.stringify(openidm.query('managed/widget',{_queryFilter:filter,_fields:['_id']}).result.map(function (r) { return r._id; })); } catch (e) { 'caught:' + e.message; }";
      const response = await runner.eval({ engine: "idm", source, bindings: { filter: row.filter as never } }, { onCall: (await fresh()).onCall });
      if (row.script === "fault") {
        expect(response.outcome, row.why).toBe("runtime_error");
        // The JVM's IDM response rebuilds `emulation` from causes, so the id is in the cause here;
        // the host callback's audit entry carries it as a fault (fault-tally.test.ts).
        expect(response.emulation?.missing?.[0], row.why).toMatch(/^openidm-query-filter: /);
        return;
      }
      if (typeof row.script === "object" && "missing" in row.script) {
        expect(response.outcome, row.why).toBe("runtime_error");
        expect(response.emulation?.missing?.[0], row.why).toBe(row.script.missing);
        return;
      }
      expect(response.outcome, JSON.stringify(response.error)).toBe("ok");
      if (row.script === "parse") expect(String(response.value), row.why).toContain("A value could not be parsed as a valid query filter");
      else expect(response.value, row.why).toBe(JSON.stringify(row.script));
    });
  });

  // E19 live probe (2026-11-06): the collection-keyed table, over REST and from a script.
  describe("probe table by collection", () => {
    const types = {
      alpha_user: { userName: "string", telephoneNumber: "string", frIndexedDate1: "string", frUnindexedDate1: "string", frIndexedDate2: "string",
        frIndexedInteger1: "number", frIndexedInteger2: "number", frUnindexedInteger1: "number",
        frIndexedMultivalued1: "array", frUnindexedMultivalued1: "array", roles: "array", frIndexedMultivalued2: "array" },
      sink: { count: "number" },
      linkedsink: { count: "number", linked: "number" },
      pinned: { count: "number" },
    } as const;
    const schema = (props: Record<string, string>) => ({ properties: Object.fromEntries(Object.entries(props).map(([k, type]) => [k, { type }])) });
    const config = (withRepo: boolean, declared: Record<string, Record<string, string>> = types) => new IdmConfig({ sync: { mappings: [] },
      managed: { objects: [...Object.entries(declared).map(([name, props]) => ({ name, schema: schema(props) })), { name: "bravo_user", schema: schema(types.alpha_user) }] },
      ...(withRepo ? { repo } : {}),
    });
    // alpha_user and bravo_user ids must be UUIDs (measured), so the table's letters are mapped.
    const uuid = (type: string, id: string) => type.endsWith("_user") ? `00000000-0000-0000-0000-${id.padStart(12, "0")}` : id;
    const letter = (id: unknown) => String(id).replace(/^00000000-0000-0000-0000-0*/, "");
    const fresh = async (withRepo = true, only?: string, given = collectionRecords, declared: Record<string, Record<string, string>> = types) => {
      const emu = new IdmEmulator(config(withRepo, declared), runner);
      for (const [type, rows] of Object.entries(given)) {
        if (only !== undefined) continue;
        for (const { _id, ...record } of rows) {
          await send(emu, { method: "PUT", path: `managed/${type}/${uuid(type, _id)}`, headers: { "If-None-Match": "*" }, body: structuredClone(record) as Document }, 201);
        }
      }
      return emu;
    };
    // An off-form row's stored value may contradict alpha_user's usual schema (a number in
    // telephoneNumber). Declaring the field with the stored value's type lets the write land, so the
    // query reaches the rule's own guard rather than the write's valid-type policy.
    const declaredFor = (row: CollectionRow) => {
      if (row.records === undefined) return types;
      const props: Record<string, string> = { ...types[row.collection as keyof typeof types] };
      for (const { _id, ...record } of row.records) {
        for (const [key, value] of Object.entries(record)) props[key] = Array.isArray(value) ? "array" : typeof value;
      }
      return { ...types, [row.collection]: props };
    };
    const forRow = (row: CollectionRow) => fresh(true, undefined, recordsFor(row), declaredFor(row));
    const cases = collectionTable.map((row) => [`${row.collection} ${row.filter}`, row] as const);
    const expectFor = (row: CollectionRow) => {
      if (!Array.isArray(row.idm) && "missing" in row.idm) expectEmulationFailure(row.idm.missing);
    };
    it.each(cases)("REST %s", async (_name, row) => {
      expectFor(row);
      const result = await (await forRow(row)).request({ method: "GET", path: `managed/${row.collection}`, query: { _queryFilter: row.filter } });
      const body = result.body as Document;
      if (Array.isArray(row.idm)) {
        expect(result.status, JSON.stringify(result.provenance)).toBe(200);
        expect(((body.result ?? []) as Document[]).map((r) => letter(r._id)), row.why).toEqual(row.idm);
      } else if ("missing" in row.idm) {
        expect(result.status, row.why).toBe(501);
        expect(result.provenance, row.why).toContainEqual({ kind: "missing", behavior: row.idm.missing });
        expect(result.emulation?.faults?.map((fault) => fault.id)).toEqual(["openidm-query-filter"]);
      } else {
        // HTML-escaped over REST, as measured (`&#39;`).
        expect(result.status, row.why).toBe(400);
        expect(body, row.why).toEqual(escapeError({ code: 400, reason: "Bad Request", message: illegalMessage(row.idm.illegal, row.idm.field) }));
        expect(result.provenance).toContainEqual({ kind: "measured", behavior: "a literal a DS integer or time attribute cannot convert is a 400 illegal filter assertion value",
          source: "docs/api/10-managed-objects.md (E19 live probe, 2026-11-06)" });
      }
    });
    it.each(cases)("script %s", async (_name, row) => {
      expectFor(row);
      const source = "try { JSON.stringify(openidm.query('managed/' + type,{_queryFilter:filter,_fields:['_id']}).result.map(function (r) { return String(r._id).replace(/^00000000-0000-0000-0000-0*/, ''); })); } catch (e) { JSON.stringify([e.name, e.message, String(e), e.javaException.getCode()]); }";
      const response = await runner.eval({ engine: "idm", source, bindings: { filter: row.filter, type: row.collection } }, { onCall: (await forRow(row)).onCall });
      if (!Array.isArray(row.idm) && "missing" in row.idm) {
        expect(response.outcome, row.why).toBe("runtime_error");
        expect(response.emulation?.missing?.[0], row.why).toBe(row.idm.missing);
        return;
      }
      expect(response.outcome, JSON.stringify(response.error)).toBe("ok");
      if (Array.isArray(row.idm)) expect(response.value, row.why).toBe(JSON.stringify(row.idm));
      else {
        // A catchable JavaException: the class prefix and the unescaped text, as measured (second
        // probe, 2026-11-06: e.name, e.message and String(e) captured separately).
        const message = `org.forgerock.json.resource.BadRequestException: ${illegalMessage(row.idm.illegal, row.idm.field)}`;
        expect(JSON.parse(String(response.value)), row.why).toEqual(["JavaException", message, `JavaException: ${message}`, 400]);
      }
    });

    // AIC converts assertions before reading records: on an empty collection the 400 and the
    // query-level refusal still happen, so a script cannot pass on an empty fixture.
    it.each([["frIndexedInteger1 eq 7.5", 400], ['frIndexedDate1 eq "notadate"', 400], ['telephoneNumber eq "(04) 9999 9999"', 501],
      ["frIndexedInteger1 ge 7.5", 501], ['frIndexedDate2 eq "2026-01-01T00:00:00Z"', 501], ['frIndexedInteger1 eq "7"', 200]])(
      "empty alpha_user: %s is %i", async (filter, status) => {
        if (status === 501) expectHarnessFault("openidm-query-filter");
        const result = await (await fresh(true, "none")).request({ method: "GET", path: "managed/alpha_user", query: { _queryFilter: filter as string } });
        expect(result.status, JSON.stringify(result.body)).toBe(status);
      });

    // How storage kind is known: the opt-in repo. Without it a generic type's coercion is refused.
    it.each([['count eq "7"', "eq"], ["count eq 7.5", "eq"]])("without repo, sink %s is refused", async (filter, op) => {
      const cause = `openidm-query-filter: query ${op} whose ${causes.type}: sink count`;
      expectEmulationFailure(cause);
      const result = await (await fresh(false)).request({ method: "GET", path: "managed/sink", query: { _queryFilter: filter } });
      expect(result.status).toBe(501);
      expect(result.provenance).toContainEqual({ kind: "missing", behavior: cause });
    });
    it("the storage kind comes from repo's mappings", () => {
      const both = new IdmConfig({ sync: { mappings: [] }, managed: { objects: [] }, repo: { resourceMapping: { explicitMapping: { "managed/x": {} }, genericMapping: { "managed/x": {}, "managed/*": {} } } } });
      expect([both.storage("x"), both.storage("y"), both.storage("alpha_user")]).toEqual(["unknown", "generic", "explicit"]);
      const noWildcard = new IdmConfig({ sync: { mappings: [] }, managed: { objects: [] }, repo: { resourceMapping: { genericMapping: { "managed/x": {} } } } });
      expect([noWildcard.storage("x"), noWildcard.storage("y")]).toEqual(["generic", "unknown"]);
      const cfg = config(true);
      // sink is covered by managed/* alone (the probe's shape, review P2-1); linkedsink has its own entry (P3-1).
      expect([cfg.storage("sink"), cfg.storage("linkedsink"), cfg.storage("pinned"), cfg.jsonStored("sink", "count"), cfg.jsonStored("linkedsink", "count"),
        cfg.jsonStored("linkedsink", "linked"), cfg.jsonStored("sink", "a/b"), cfg.jsonStored("pinned", "count"), cfg.jsonStored("alpha_user", "custom_n")])
        .toEqual(["generic", "generic", "explicit", "wildcard", "own", false, false, false, false]);
      expect(config(false).storage("sink")).toBe("unknown");
      expect(() => new IdmConfig({ sync: { mappings: [] }, managed: { objects: [] }, repo: [] as never })).toThrow(/repo must be a plain object/);
    });

    // Provenance: what the probe measured is labelled measured, the inferred neighbours assumed.
    const PROBE = "docs/api/10-managed-objects.md (E19 live probe, 2026-11-06)";
    it.each([
      ["alpha_user", 'telephoneNumber eq "+614 9999 9999"', { kind: "measured", behavior: "eq on alpha_user telephoneNumber ignores spaces, hyphens and + (telephoneNumberMatch)", source: PROBE }],
      ["alpha_user", 'telephoneNumber co "+9999"', { kind: "measured", behavior: "co on alpha_user telephoneNumber ignores spaces, hyphens and + (telephoneNumberMatch)", source: PROBE }],
      ["alpha_user", 'frUnindexedDate1 lt "2026-01-01T00:00:00Z"', { kind: "measured", behavior: "lt on alpha_user frUnindexedDate1 compares instants across offsets", source: PROBE }],
      ["alpha_user", "frIndexedInteger1 eq 7.0", { kind: "measured", behavior: "eq on alpha_user frIndexedInteger1 converts an integral literal of either JSON type to the integer", source: PROBE }],
      ["alpha_user", "frIndexedInteger1 ge 7.0", { kind: "measured", behavior: "ge on alpha_user frIndexedInteger1 converts an integral literal of either JSON type to the integer", source: PROBE }],
      ["alpha_user", "frIndexedInteger1 gt 7", { kind: "assumed", behavior: "gt on alpha_user frIndexedInteger1 compares an integral literal numerically, like eq and ge" }],
      ["alpha_user", "roles pr", { kind: "measured", behavior: "pr reads an explicitly written [] on alpha_user roles as absent", source: PROBE }],
      ["sink", 'count eq "7"', { kind: "measured", behavior: "eq on a generic (JSON-stored) type: a quoted literal never matches a number", source: PROBE }],
      ["sink", "count gt 6.5", { kind: "measured", behavior: "gt on a generic (JSON-stored) type compares numbers by value, decimals included", source: PROBE }],
      ["sink", "count le 7.5", { kind: "assumed", behavior: "le on a generic (JSON-stored) type compares numbers by value, like eq and gt" }],
      ["linkedsink", 'count eq "7"', { kind: "assumed", behavior: "eq on a generic (JSON-stored) type: a quoted literal never matches a number, assumed for a type with its own genericMapping entry like managed/*" }],
      ["linkedsink", "count eq 7.0", { kind: "assumed", behavior: "eq on a generic (JSON-stored) type compares numbers by value, decimals included, assumed for a type with its own genericMapping entry like managed/*" }],
    ])("%s %s is labelled", async (type, filter, label) => {
      const result = await (await fresh()).request({ method: "GET", path: `managed/${type}`, query: { _queryFilter: filter } });
      expect(result.status, JSON.stringify(result.provenance)).toBe(200);
      expect(result.provenance).toContainEqual(label);
    });
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
      const props = { userName: { type: "string" }, mail: { type: "string" }, givenName: { type: "string" }, sn: { type: "string" }, city: { type: "string" } };
      const emu = new IdmEmulator(new IdmConfig({ sync: { mappings: [] }, managed: { objects: [{ name: "alpha_user", schema: { properties: props } }, { name: "bravo_user", schema: { properties: props } }] } }), runner);
      for (const type of ["alpha_user", "bravo_user"]) await send(emu, { method: "PUT", path: `managed/${type}/${user}`, headers: { "If-None-Match": "*" }, body: { userName: "ua", mail: "ma@example.com", givenName: "alice", sn: "doe", city: "perth" } }, 201);
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
      ['mail sw "MA@"', "alpha_user", "openidm-query-filter: query sw whose match turns on letter case: alpha_user mail"],
      ['mail co "EXAMPLE"', "alpha_user", "openidm-query-filter: query co whose match turns on letter case: alpha_user mail"],
      ['mail eq "MA@example.com"', "bravo_user", "openidm-query-filter: query eq whose match turns on letter case: bravo_user mail"],
      ['userName eq "UA"', "bravo_user", "openidm-query-filter: query eq whose match turns on letter case: bravo_user userName"],
    ])("%s on %s is missing: case sensitivity there is unmeasured", async (filter, type, cause) => {
      expectEmulationFailure(cause);
      const result = await (await users()).request({ method: "GET", path: `managed/${type}`, query: { _queryFilter: filter } });
      expect(result.status).toBe(501);
      expect(result.provenance).toContainEqual({ kind: "missing", behavior: cause });
    });
    it("a non-ASCII case difference on a measured field is missing", async () => {
      const cause = "openidm-query-filter: query eq whose match turns on letter case: alpha_user userName";
      expectEmulationFailure(cause);
      const emu = await users();
      await send(emu, { method: "PUT", path: "managed/alpha_user/00000000-0000-0000-0000-0000000000a2", headers: { "If-None-Match": "*" }, body: { userName: "émile" } }, 201);
      const result = await emu.request({ method: "GET", path: "managed/alpha_user", query: { _queryFilter: 'userName eq "Émile"' } });
      expect(result.status).toBe(501);
    });
    // 3bddde0 (doc 10, 2026-11-04): five more alpha_user properties fold under eq; gt/ge/lt on
    // userName fold; eq on mail/userName ignores leading and trailing spaces, not interior ones.
    const query = async (emu: IdmEmulator, type: string, filter: string) => emu.request({ method: "GET", path: `managed/${type}`, query: { _queryFilter: filter } });
    const label = (kind: string, behavior: string) => kind === "measured" ? { kind, behavior, source: "docs/api/10-managed-objects.md (eq/gt/lt ignore case and edge spaces, 2026-11-04)" } : { kind, behavior };
    const PROBED = label("measured", "eq on alpha_user givenName, sn, description, frIndexedString1, frUnindexedString1 ignores letter case");
    const SPACE = label("measured", "eq on alpha_user mail and userName ignores leading and trailing spaces");
    const ORDER = label("measured", "gt, ge and lt on alpha_user userName ignore letter case");
    const ORDERING = label("assumed", "string ordering compares UTF-16 code units after the case and space rules");
    it.each([
      ['givenName eq "ALICE"', 1, [PROBED]],
      ['sn eq "Doe"', 1, [PROBED]],
      ['city eq "Perth"', 1, [label("assumed", "eq on alpha_user city ignores letter case: unprobed, assumed like the probed properties")]],
      ['mail eq " ma@example.com"', 1, [SPACE]],
      ['userName eq "ua  "', 1, [SPACE]],
      ['mail eq " MA@example.com "', 1, [SPACE, CASE]],
      ['city eq " perth"', 1, [label("assumed", "eq on alpha_user ignores leading and trailing spaces like eq on mail and userName")]],
      // Ordering: code-point order would answer the other way on each of the first three.
      ['userName gt "UA"', 0, [ORDERING]],
      ['userName ge "UB"', 0, [ORDER, ORDERING]],
      ['userName lt "UB"', 1, [ORDER, ORDERING]],
      ['userName le "UA"', 1, [label("assumed", "le on alpha_user userName ignores letter case: unprobed, assumed like the probed properties"), ORDERING]],
      ['userName gt "ta"', 1, [ORDERING]],
    ])("alpha_user %s answers %i, labelled", async (filter, count, labels) => {
      const result = await query(await users(), "alpha_user", filter as string);
      expect(result.status, JSON.stringify(result.provenance)).toBe(200);
      expect((result.body as Document).resultCount).toBe(count);
      for (const l of labels as object[]) expect(result.provenance).toContainEqual(l);
    });
    it.each([['mail eq "ma@ example.com"'], ['mail eq "ma@example.com"']])("alpha_user %s needs no space rule", async (filter) => {
      const result = await query(await users(), "alpha_user", filter);
      expect((result.body as Document).resultCount).toBe(filter.includes("@ ") ? 0 : 1);
      expect(result.provenance).not.toContainEqual(SPACE);
    });
    it.each([
      ['mail eq " ma@example.com"', "bravo_user", "openidm-query-filter: query eq whose match turns on leading, trailing or repeated spaces or non-U+0020 whitespace: bravo_user mail"],
      ['mail sw " ma"', "alpha_user", "openidm-query-filter: query sw whose match turns on leading, trailing or repeated spaces or non-U+0020 whitespace: alpha_user mail"],
      ['userName gt "UA"', "bravo_user", "openidm-query-filter: query gt whose order turns on letter case: bravo_user userName"],
    ])("%s on %s is missing", async (filter, type, cause) => {
      expectEmulationFailure(cause);
      const result = await query(await users(), type, filter);
      expect(result.status).toBe(501);
      expect(result.provenance).toContainEqual({ kind: "missing", behavior: cause });
    });
    it("a run of interior spaces is missing on a measured field: whether DS collapses it is unmeasured", async () => {
      const cause = "openidm-query-filter: query eq whose match turns on leading, trailing or repeated spaces or non-U+0020 whitespace: alpha_user givenName";
      expectEmulationFailure(cause);
      const emu = await users();
      await send(emu, { method: "PUT", path: "managed/alpha_user/00000000-0000-0000-0000-0000000000a4", headers: { "If-None-Match": "*" }, body: { givenName: "mary ann" } }, 201);
      const result = await query(emu, "alpha_user", 'givenName eq "mary  ann"');
      expect(result.status).toBe(501);
      expect(result.provenance).toContainEqual({ kind: "missing", behavior: cause });
    });
    it("an ordering whose answer turns on the fold direction is missing", async () => {
      // "u_" vs "uA": lower-cased "u_" < "ua", upper-cased "U_" > "UA".
      const cause = "openidm-query-filter: query gt whose order turns on letter case: alpha_user userName";
      expectEmulationFailure(cause);
      const emu = await users();
      await send(emu, { method: "PUT", path: "managed/alpha_user/00000000-0000-0000-0000-0000000000a3", headers: { "If-None-Match": "*" }, body: { userName: "u_" } }, 201);
      expect((await query(emu, "alpha_user", 'userName gt "uA"')).status).toBe(501);
    });
    // R5-2a: an unmeasured leaf refuses only when the record's whole answer turns on it.
    it.each([
      ['givenName eq "ALICE" and sn eq "smith"', 0], ['sn eq "smith" and givenName eq "ALICE"', 0],
      ['givenName eq "ALICE" or sn eq "doe"', 1], ['sn eq "doe" or givenName eq "ALICE"', 1],
      ['!(givenName eq "ALICE" and sn eq "smith")', 1],
    ])("bravo_user %s answers %i in either operand order", async (filter, count) => {
      const result = await query(await users(), "bravo_user", filter as string);
      expect(result.status, JSON.stringify(result.provenance)).toBe(200);
      expect((result.body as Document).resultCount).toBe(count);
    });
    it.each([['givenName eq "ALICE" and sn eq "doe"'], ['sn eq "doe" and givenName eq "ALICE"'], ['givenName eq "ALICE" or sn eq "smith"']])("bravo_user %s turns on case and is missing", async (filter) => {
      const cause = "openidm-query-filter: query eq whose match turns on letter case: bravo_user givenName";
      expectEmulationFailure(cause);
      const result = await query(await users(), "bravo_user", filter);
      expect(result.status).toBe(501);
      expect(result.provenance).toContainEqual({ kind: "missing", behavior: cause });
    });
    // R6-1: negation and array elements are the two arms that could turn `unknown` into a definite answer.
    it("bravo_user !(case-only leaf) is missing, not the negation of a guess", async () => {
      const cause = "openidm-query-filter: query eq whose match turns on letter case: bravo_user givenName";
      expectEmulationFailure(cause);
      const result = await query(await users(), "bravo_user", '!(givenName eq "ALICE")');
      expect(result.status).toBe(501);
      expect(result.provenance).toContainEqual({ kind: "missing", behavior: cause });
    });
    it("a case-only match against an array element on bravo_user is missing, not dropped", async () => {
      const cause = "openidm-query-filter: query eq whose match turns on letter case: bravo_user tags";
      expectEmulationFailure(cause);
      const emu = new IdmEmulator(new IdmConfig({ sync: { mappings: [] }, managed: { objects: [{ name: "bravo_user", schema: { properties: { tags: { type: "array", items: { type: "string" } } } } }] } }), runner);
      await send(emu, { method: "PUT", path: `managed/bravo_user/${user}`, headers: { "If-None-Match": "*" }, body: { tags: ["beta", "Alpha"] } }, 201);
      const result = await query(emu, "bravo_user", 'tags eq "alpha"');
      expect(result.status).toBe(501);
      expect(result.provenance).toContainEqual({ kind: "missing", behavior: cause });
      // Control: an exact element match is definite whatever the other element would do.
      expect(((await query(emu, "bravo_user", 'tags eq "beta"')).body as Document).resultCount).toBe(1);
    });
    // R6-2: "space" was U+0020 only, and no case variant differed on ß. Neither is measured, so an
    // answer that turns on other whitespace or on a non-ASCII fold is missing, not a definite guess.
    const SPACE_CAUSE = (field: string) => `openidm-query-filter: query eq whose match turns on leading, trailing or repeated spaces or non-U+0020 whitespace: alpha_user ${field}`;
    it.each([
      ["a trailing no-break space", "mail", "ma@example.com", "ma@example.com\u00a0"],
      ["a trailing tab", "mail", "ma@example.com", "ma@example.com\t"],
      ["an interior no-break space", "givenName", "mary ann", "mary\u00a0ann"],
      ["a leading ideographic space", "userName", "ub", "\u3000ub"],
    ])("alpha_user eq differing by %s is missing", async (_what, field, stored, expected) => {
      const cause = SPACE_CAUSE(field);
      expectEmulationFailure(cause);
      const emu = await users();
      await send(emu, { method: "PUT", path: "managed/alpha_user/00000000-0000-0000-0000-0000000000a5", headers: { "If-None-Match": "*" }, body: { [field]: stored } }, 201);
      const result = await query(emu, "alpha_user", `${field} eq ${JSON.stringify(expected)}`);
      expect(result.status).toBe(501);
      expect(result.provenance).toContainEqual({ kind: "missing", behavior: cause });
    });
    it.each([["givenName", "stra\u00dfe", "STRASSE"], ["givenName", "STRASSE", "stra\u00dfe"]])("alpha_user %s %s eq %s turns on a non-ASCII fold and is missing", async (field, stored, expected) => {
      const cause = "openidm-query-filter: query eq whose match turns on letter case: alpha_user givenName";
      expectEmulationFailure(cause);
      const emu = await users();
      await send(emu, { method: "PUT", path: "managed/alpha_user/00000000-0000-0000-0000-0000000000a6", headers: { "If-None-Match": "*" }, body: { [field]: stored } }, 201);
      const result = await query(emu, "alpha_user", `${field} eq ${JSON.stringify(expected)}`);
      expect(result.status).toBe(501);
      expect(result.provenance).toContainEqual({ kind: "missing", behavior: cause });
    });
    // R7-4: on a type with no measured rule the wide variants are what make a trailing NBSP undecided.
    it("bravo_user eq differing only by a trailing no-break space is missing, not a definite 0", async () => {
      const cause = "openidm-query-filter: query eq whose match turns on leading, trailing or repeated spaces or non-U+0020 whitespace: bravo_user mail";
      expectEmulationFailure(cause);
      const result = await query(await users(), "bravo_user", 'mail eq "ma@example.com\u00a0"');
      expect(result.status).toBe(501);
      expect(result.provenance).toContainEqual({ kind: "missing", behavior: cause });
    });
    it("controls: a no-break space or ß on both sides is definite", async () => {
      const emu = await users();
      await send(emu, { method: "PUT", path: "managed/alpha_user/00000000-0000-0000-0000-0000000000a7", headers: { "If-None-Match": "*" }, body: { givenName: "stra\u00dfe\u00a0x" } }, 201);
      const result = await query(emu, "alpha_user", 'givenName eq "STRA\u00dfE\u00a0X"');
      expect(result.status, JSON.stringify(result.provenance)).toBe(200);
      expect((result.body as Document).resultCount).toBe(1);
    });
    // R6-3: the refusal names the leaf that left the answer undecided, not the first unknown one.
    it("the missing message names the deciding leaf, not one an or already made irrelevant", async () => {
      const cause = "openidm-query-filter: query eq whose match turns on letter case: bravo_user city";
      expectEmulationFailure(cause);
      const result = await query(await users(), "bravo_user", '(givenName eq "ALICE" or sn eq "doe") and city eq "PERTH"');
      expect(result.status).toBe(501);
      expect(result.provenance).toContainEqual({ kind: "missing", behavior: cause });
    });
    it("the reviewer's example (case-swapped givenName, wrong sn) is measured on alpha_user now, in both operand orders", async () => {
      for (const filter of ['givenName eq "Alice" and sn eq "smith"', 'sn eq "smith" and givenName eq "Alice"']) {
        const result = await query(await users(), "alpha_user", filter);
        expect(result.status).toBe(200);
        expect((result.body as Document).resultCount).toBe(0);
      }
    });
    it("a case-only eq on an unprobed managed type is missing", async () => {
      const cause = "openidm-query-filter: query eq whose match turns on letter case: widget text";
      expectEmulationFailure(cause);
      const result = await seeded.fork().request({ method: "GET", path: "managed/widget", query: { _queryFilter: 'text eq "ALPHA"' } });
      expect(result.status).toBe(501);
    });
  });

  // E19 round 5. Each row drives the real emulator; the tally id rides every refusal.
  describe("round 5: types, DS matching rules, labels, injected arrays", () => {
    const id = "00000000-0000-0000-0000-0000000000b1";
    const refusal = (op: string, what: string, type: string, field: string) => `openidm-query-filter: query ${op} whose ${what}: ${type} ${field}`;
    const TYPE = "match compares different JSON types, or an object or null element, which nothing measured";
    const RULE = causes.rule;
    const PROBE = "docs/api/10-managed-objects.md (E19 live probe, 2026-11-06)";
    const CONVERTS = "eq on alpha_user frIndexedInteger1 converts an integral literal of either JSON type to the integer";
    const people = async () => {
      const props = { userName: { type: "string" }, givenName: { type: "string" }, telephoneNumber: { type: "string" },
        frIndexedDate1: { type: "string" }, frUnindexedDate2: { type: "string" }, frIndexedInteger1: { type: "number" },
        frIndexedInteger2: { type: "number" }, custom_n: { type: "number" } };
      const emu = new IdmEmulator(new IdmConfig({ sync: { mappings: [] }, managed: { objects: [{ name: "alpha_user", schema: { properties: props } }, { name: "bravo_user", schema: { properties: props } }] } }), runner);
      for (const type of ["alpha_user", "bravo_user"]) await send(emu, { method: "PUT", path: `managed/${type}/${id}`, headers: { "If-None-Match": "*" }, body: {
        userName: "ub", givenName: "alice", telephoneNumber: "+61-2-9999-9999", frIndexedDate1: "2026-01-01T10:00:00+11:00",
        frUnindexedDate2: "2026-01-01T00:00:00Z", frIndexedInteger1: 7, frIndexedInteger2: 7, custom_n: 7 } }, 201);
      return emu;
    };
    const query = async (emu: IdmEmulator, type: string, filter: string) => emu.request({ method: "GET", path: `managed/${type}`, query: { _queryFilter: filter } });

    // P2-1: measured on alpha_user frIndexedInteger1 only; P3-5: the unquoted form is labelled too.
    it.each([
      ['frIndexedInteger1 eq "7"', 1, { kind: "measured", behavior: CONVERTS, source: PROBE }],
      ['frIndexedInteger1 eq "8"', 0, { kind: "measured", behavior: CONVERTS, source: PROBE }],
      ['frIndexedInteger2 eq "7"', 1, { kind: "assumed", behavior: "eq on alpha_user frIndexedInteger2 matches a quoted integer like frIndexedInteger1" }],
      ["frIndexedInteger1 eq 7", 1, { kind: "measured", behavior: CONVERTS, source: PROBE }],
      ["frIndexedInteger2 gt 6", 1, { kind: "assumed", behavior: "gt on a number property compares JSON numbers numerically" }],
    ])("alpha_user %s answers %i, labelled", async (filter, count, label) => {
      const result = await query(await people(), "alpha_user", filter as string);
      expect(result.status, JSON.stringify(result.provenance)).toBe(200);
      expect((result.body as Document).resultCount).toBe(count);
      expect(result.provenance).toContainEqual(label);
    });
    it.each([
      ['custom_n eq "7"', "alpha_user", "eq", "custom_n", TYPE, "a custom_* property is JSON in DS: coercion unmeasured"],
      ['frIndexedInteger1 eq "7"', "bravo_user", "eq", "frIndexedInteger1", TYPE, "measured on alpha_user only"],
      ['!(frIndexedInteger1 eq "7")', "bravo_user", "eq", "frIndexedInteger1", TYPE, "and not its negation either"],
      ["frIndexedInteger1 eq 7.0", "bravo_user", "eq", "frIndexedInteger1", TYPE, "P2-2: decided on alpha_user since 2026-11-06, not on bravo_user"],
      // P2-3: the alpha_user telephone and date-1 rules were measured on 2026-11-06 (the probe table
      // above); bravo_user and date 2-5 were not.
      ['telephoneNumber eq "+61 2 9999 9999"', "bravo_user", "eq", "telephoneNumber", RULE, "the same DS attribute on bravo_user"],
      ['frUnindexedDate2 eq "2026-01-01T11:00:00+11:00"', "alpha_user", "eq", "frUnindexedDate2", RULE, "the same instant spelt with an offset"],
    ])("%s on %s is refused with the fault id", async (filter, type, op, field, what) => {
      const cause = refusal(op, what, type, field);
      expectEmulationFailure(cause);
      const result = await query(await people(), type, filter);
      expect(result.status).toBe(501);
      expect(result.provenance).toContainEqual({ kind: "missing", behavior: cause });
      expect(result.emulation?.faults?.map((fault) => fault.id)).toEqual(["openidm-query-filter"]);
    });
    it.each([["telephoneNumber pr", 1], ['givenName eq "ALICE"', 1], ['userName eq "ub" and !(frIndexedDate1 pr)', 0]])("control: alpha_user %s is decided", async (filter, count) => {
      const result = await query(await people(), "alpha_user", filter as string);
      expect(result.status, JSON.stringify(result.provenance)).toBe(200);
      expect((result.body as Document).resultCount).toBe(count);
    });
    it("a refused field decides nothing in the AM lane's collection either", async () => {
      // The emulator's own script host call: same evaluator, same refusal.
      const cause = refusal("eq", RULE, "alpha_user", "telephoneNumber");
      expectEmulationFailure(cause);
      const emu = await people();
      const response = await runner.eval({ engine: "idm", source: "openidm.query('managed/alpha_user',{_queryFilter:filter}).resultCount;", bindings: { filter: 'telephoneNumber eq "x"' } }, { onCall: emu.onCall });
      expect(response.outcome).toBe("runtime_error");
      expect(response.emulation?.missing?.[0]).toBe(cause);
    });

    // P3-5: number ordering and boolean eq carry a label.
    it.each([
      ["count gt 1", { kind: "assumed", behavior: "gt on a number property compares JSON numbers numerically" }],
      ["flag eq true", { kind: "assumed", behavior: "eq on a boolean property compares JSON booleans" }],
      ["count eq 2", { kind: "assumed", behavior: "eq on a number property compares JSON numbers numerically" }],
    ])("widget %s is labelled", async (filter, label) => {
      const result = await query(seeded.fork(), "widget", filter as string);
      expect(result.status).toBe(200);
      expect(result.provenance).toContainEqual(label);
    });

    // P3-6: an array the emulator injected reads as absent, through a PATCH and a fork; one a
    // write names (even as []) is the unmeasured explicit case again.
    it("an injected [] stays absent until a write names it", async () => {
      const emu = emulator(runner);
      await create(emu, { text: "t" }, "w");
      const absent = async (target: IdmEmulator) => ((await query(target, "widget", "!(list pr)")).body as Document).resultCount;
      expect(await absent(emu)).toBe(1);
      await send(emu, { method: "PATCH", path: "managed/widget/w", body: [{ operation: "replace", field: "/text", value: "u" }] });
      expect(await absent(emu)).toBe(1);
      expect(await absent(emu.fork())).toBe(1);
      await send(emu, { method: "PUT", path: "managed/widget/w", body: { text: "v" } });
      expect(await absent(emu)).toBe(1);
      // A PATCH to [] over the injected [] is a measured no-op, so write a value first.
      await send(emu, { method: "PATCH", path: "managed/widget/w", body: [{ operation: "replace", field: "/list", value: ["q"] }] });
      expect(await absent(emu)).toBe(0);
      await send(emu, { method: "PATCH", path: "managed/widget/w", body: [{ operation: "replace", field: "/list", value: [] }] });
      const cause = "openidm-query-filter: query pr whose answer for an empty string, array or object is unmeasured: widget list";
      expectEmulationFailure(cause);
      expect((await query(emu, "widget", "!(list pr)")).status).toBe(501);
    });
  });
});
