/** The shared `_queryFilter` table through the grammar and the script tester's openidm.query. */
import { createRequire } from "node:module";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { parseHarvest } from "../../src/bindings/harvest.ts";
import { mockPreamble, withHarvest } from "../../src/bindings/preamble.ts";
import { expectHarnessFault } from "../../src/harness/emulation-guard.ts";
import { bindingsCrestFilterPath } from "../../src/paths.ts";
import { RhinoRunner } from "../../src/runner.ts";
import { type Answer, type CollectionRow, type Illegal, collectionRecords, collectionTable, filterTable, illegalMessage, records } from "../crest-filter-table.ts";
import type { JsonObject } from "../../src/case/types.ts";
import { loadBehaviour, sandboxFaults } from "./load-behaviour.ts";

const grammar = createRequire(import.meta.url)(bindingsCrestFilterPath) as {
  parse(input: unknown): { ast?: unknown; error?: string };
};
const given = { managed: { "managed/widget": records.map((record) => structuredClone(record) as JsonObject) } };
const parseText = (filter: unknown) =>
  `rhino-local: openidm.query: The value '${String(filter)}' for parameter '_queryFilter' could not be parsed as a valid query filter`;
const rows = (answer: Answer) => (Array.isArray(answer) ? answer : []);
/** AM's openidm.query 400 (second probe, 2026-11-06, docs/api/12): REST's escaping, the adapter's class, no suffix. */
const amIllegal = (row: Illegal) =>
  `org.forgerock.openam.scripting.wrappers.ResourceExceptionScriptAdapter: ${illegalMessage(row.illegal, row.field).replace(/'/g, "&#39;")}`;
const isIllegal = (answer: CollectionRow["am"]): answer is Illegal => typeof answer === "object" && !Array.isArray(answer);

describe("the shared grammar", () => {
  it.each(filterTable.map((row) => [JSON.stringify(row.filter), row] as const))("%s", (_name, row) => {
    const parsed = grammar.parse(row.filter);
    expect(parsed.error ?? "ast", row.why).toBe(row.grammar);
  });
});

describe("openidm.query in node:vm", () => {
  it.each(filterTable.map((row) => [JSON.stringify(row.filter), row] as const))("%s", (_name, row) => {
    const sandbox = loadBehaviour(given);
    const openidm = sandbox.openidm as { query: (r: string, p: unknown) => { result: Array<{ _id: string }> } };
    const call = () => openidm.query("managed/widget", { _queryFilter: row.filter }).result.map((r) => r._id);
    if (row.am === "parse") {
      expect(call, row.why).toThrow(parseText(row.filter));
      expect(sandboxFaults(sandbox)).toEqual([]);
    } else if (row.am === "fault") {
      expect(call, row.why).toThrow(/unmocked filter/);
      expect(sandboxFaults(sandbox).map((f) => f.id)).toEqual(["openidm-query-filter"]);
    } else {
      expect(call(), row.why).toEqual(row.am);
      expect(sandboxFaults(sandbox)).toEqual([]);
    }
  });
});

// E19 round 5: rows the widget table cannot hold, because they turn on the collection's name.
const users = { managed: { "managed/alpha_user": [{ _id: "u1", userName: "ub", frIndexedInteger1: 7, telephoneNumber: "+61-2-9999-9999", frIndexedDate1: "2026-01-01T10:00:00+11:00" }],
  "managed/widget": [{ _id: "w1", telephoneNumber: "+61-2-9999-9999" }] } };
const userRows: Array<[string, string, string[] | "fault", string]> = [
  // Measured through AM's openidm.query (second probe, 2026-11-06): the field rules apply here too.
  ['frIndexedInteger1 eq "7"', "managed/alpha_user", ["u1"], "quoted integer on frIndexedInteger1 (measured through AM)"],
  ["frIndexedInteger1 eq 7", "managed/alpha_user", ["u1"], "control: an unquoted integer is decided"],
  ['telephoneNumber eq "+61 2 9999 9999"', "managed/alpha_user", ["u1"], "telephoneNumberMatch ignores spaces, hyphens and +"],
  ['frIndexedDate1 gt "2026-01-01T00:00:00Z"', "managed/alpha_user", [], "GeneralizedTime compares instants: u1 is 2025-12-31T23:00Z"],
  ['frIndexedDate1 lt "2026-01-01T00:00:00Z"', "managed/alpha_user", ["u1"], "control: the string order would say not lt"],
  ['telephoneNumber eq "+61 2 9999 9999"', "managed/widget", [], "control: a generic type's property is not a DS user attribute"],
  ['userName eq "ub"', "managed/alpha_user", ["u1"], "control: other alpha_user fields are decided"],
];
describe("openidm.query by collection (node:vm)", () => {
  it.each(userRows)("%s on %s", (filter, collection, answer, why) => {
    const sandbox = loadBehaviour(structuredClone(users));
    const openidm = sandbox.openidm as { query: (r: string, p: unknown) => { result: Array<{ _id: string }> } };
    const call = () => openidm.query(collection, { _queryFilter: filter }).result.map((r) => r._id);
    if (answer === "fault") {
      expect(call, why).toThrow(/unmocked filter/);
      expect(sandboxFaults(sandbox).map((f) => f.id)).toEqual(["openidm-query-filter"]);
    } else {
      expect(call(), why).toEqual(answer);
      expect(sandboxFaults(sandbox)).toEqual([]);
    }
  });
});

// E19 live probes (2026-11-06): the collection-keyed table. The second probe measured this binding
// on the alpha_user rules, so it decides them and throws its own 400; the rest still fault, even on
// an empty collection.
const byCollection = { managed: Object.fromEntries(Object.entries(collectionRecords).map(([type, rows]) => [`managed/${type}`, rows])) };
const collectionCases = collectionTable.map((row) => [`${row.collection} ${row.filter}`, row] as const);
// A leaf whose field rule this lane cannot apply is refused before any record is read, as AIC
// converts assertions before reading: an empty collection must not pass vacuously. Record-level
// unknowns (pr on [], a quoted literal on a number) do give [] there, as AIC does.
const queryLevel: Array<[string, string, CollectionRow["am"]]> = [
  ["alpha_user", 'telephoneNumber eq "(04) 9999 9999"', "fault"], ["alpha_user", 'frIndexedDate1 ge "2025-12-31T23:00:00Z"', "fault"],
  ["alpha_user", "frIndexedInteger1 eq 7.0e0", "fault"], ["alpha_user", "frIndexedInteger2 eq 7.0", "fault"],
  ["bravo_user", 'telephoneNumber eq "+614-9999-9999"', "fault"], ["alpha_user", 'userName eq "u" or frIndexedDate2 eq "x"', "fault"],
  ["alpha_user", 'frIndexedInteger1 eq "abc"', { illegal: "abc", field: "/frIndexedInteger1" }],
  ["alpha_user", 'frIndexedDate1 eq "notadate"', { illegal: "notadate", field: "/frIndexedDate1" }],
];
const emptyCases = queryLevel.map(([collection, filter, am]) => [`${collection} ${filter}`, { collection, filter, am, idm: [], why: "query-level" } as CollectionRow] as const);
/** The thrown error's measured surface: name, message, String(e), and the adapter's code and reason. */
function expectAmIllegal(call: () => unknown, row: Illegal, why: string) {
  let caught: unknown;
  try {
    call();
  } catch (error) {
    caught = error;
  }
  const e = caught as Error & { javaException: { getCode(): number; getReason(): string } };
  const message = amIllegal(row);
  expect([e?.name, e?.message, String(e), e?.javaException?.getCode(), e?.javaException?.getReason(), String(e?.javaException)], why)
    .toEqual(["JavaException", message, `JavaException: ${message}`, 400, "Bad Request", message]);
  // AIC's enumerable keys are fileName,lineNumber (docs/api/12): the adapter is not among them.
  expect(Object.keys(e), why).not.toContain("javaException");
}
describe("openidm.query by collection, probe table (node:vm)", () => {
  const run = (row: CollectionRow, given: unknown) => {
    const sandbox = loadBehaviour(structuredClone(given) as never);
    const openidm = sandbox.openidm as { query: (r: string, p: unknown) => { result: Array<{ _id: string }> } };
    return { sandbox, call: () => openidm.query(`managed/${row.collection}`, { _queryFilter: row.filter }).result.map((r) => r._id) };
  };
  it.each(collectionCases)("%s", (_name, row) => {
    const { sandbox, call } = run(row, byCollection);
    if (isIllegal(row.am)) {
      expectAmIllegal(call, row.am, row.why);
      expect(sandboxFaults(sandbox)).toEqual([]);
    } else if (row.am === "fault") {
      expect(call, row.why).toThrow(/unmocked filter/);
      expect(sandboxFaults(sandbox).map((f) => f.id)).toEqual(["openidm-query-filter"]);
    } else {
      expect(call(), row.why).toEqual(row.am);
      expect(sandboxFaults(sandbox)).toEqual([]);
    }
  });
  it.each(emptyCases)("empty collection: %s still refuses", (_name, row) => {
    const { sandbox, call } = run(row, { managed: { [`managed/${row.collection}`]: [] } });
    if (isIllegal(row.am)) {
      expectAmIllegal(call, row.am, row.why);
      expect(sandboxFaults(sandbox)).toEqual([]);
      return;
    }
    expect(call, row.why).toThrow(/unmocked filter/);
    expect(sandboxFaults(sandbox).map((f) => f.id)).toEqual(["openidm-query-filter"]);
  });
  it.each([["sink", 'count eq "7"'], ["alpha_user", "frIndexedMultivalued1 pr"], ["alpha_user", "frIndexedInteger1 eq 7"],
    ["alpha_user", 'telephoneNumber eq "61499999999"'], ["alpha_user", 'frIndexedDate1 lt "2026-01-01T00:00:00Z"']])(
    "control: empty %s with %s answers []", (collection, filter) => {
      const { sandbox, call } = run({ collection, filter, am: [], idm: [], why: "" }, { managed: { [`managed/${collection}`]: [] } });
      expect(call()).toEqual([]);
      expect(sandboxFaults(sandbox)).toEqual([]);
    });
});

describe("openidm.query on the JVM", () => {
  let runner: RhinoRunner;
  beforeAll(async () => {
    runner = await RhinoRunner.spawn();
  }, 60_000);
  afterAll(async () => {
    await runner?.close();
  }, 30_000);

  it.each(filterTable.map((row) => [JSON.stringify(row.filter), row] as const))("%s", async (_name, row) => {
    // The script catches, so a fault can only show up through the recorder.
    const script = `try { nodeState.putShared("r", JSON.stringify(openidm.query("managed/widget", { _queryFilter: ${JSON.stringify(row.filter)} }).result.map(function (r) { return r._id; }))); } catch (e) { nodeState.putShared("r", "caught:" + e.message); }`;
    if (row.am === "fault") expectHarnessFault("openidm-query-filter");
    const response = await runner.eval({ preamble: mockPreamble(given), source: withHarvest(script) });
    if (row.am === "fault") {
      expect(response.outcome, row.why).toBe("runtime_error");
      expect(response.emulation?.faults?.map((f) => f.id)).toEqual(["openidm-query-filter"]);
      return;
    }
    expect(response.outcome, JSON.stringify(response.error)).toBe("ok");
    const shared = parseHarvest(response.value as string).sharedState.final as Record<string, unknown>;
    expect(shared.r, row.why).toBe(row.am === "parse" ? `caught:${parseText(row.filter)}` : JSON.stringify(rows(row.am)));
  });

  it.each(collectionCases)("probe table: %s", async (_name, row) => {
    const script = `try { nodeState.putShared("r", JSON.stringify(openidm.query(${JSON.stringify(`managed/${row.collection}`)}, { _queryFilter: ${JSON.stringify(row.filter)} }).result.map(function (r) { return r._id; }))); } catch (e) { nodeState.putShared("r", JSON.stringify(["caught", e.name, e.message, String(e), e.javaException ? e.javaException.getCode() : null, e.javaException ? e.javaException.getReason() : null])); }`;
    if (row.am === "fault") expectHarnessFault("openidm-query-filter");
    const response = await runner.eval({ preamble: mockPreamble(byCollection as never), source: withHarvest(script) });
    if (row.am === "fault") {
      expect(response.outcome, row.why).toBe("runtime_error");
      expect(response.emulation?.faults?.map((f) => f.id)).toEqual(["openidm-query-filter"]);
      return;
    }
    expect(response.outcome, JSON.stringify(response.error)).toBe("ok");
    const r = (parseHarvest(response.value as string).sharedState.final as Record<string, unknown>).r;
    if (isIllegal(row.am)) {
      const message = amIllegal(row.am);
      expect(JSON.parse(String(r)), row.why).toEqual(["caught", "JavaException", message, `JavaException: ${message}`, 400, "Bad Request"]);
      return;
    }
    expect(r, row.why).toBe(JSON.stringify(row.am));
  });

  it.each(userRows)("by collection: %s on %s", async (filter, collection, answer, why) => {
    const script = `try { nodeState.putShared("r", JSON.stringify(openidm.query(${JSON.stringify(collection)}, { _queryFilter: ${JSON.stringify(filter)} }).result.map(function (r) { return r._id; }))); } catch (e) { nodeState.putShared("r", "caught:" + e.message); }`;
    if (answer === "fault") expectHarnessFault("openidm-query-filter");
    const response = await runner.eval({ preamble: mockPreamble(users), source: withHarvest(script) });
    if (answer === "fault") {
      expect(response.outcome, why).toBe("runtime_error");
      expect(response.emulation?.faults?.map((f) => f.id)).toEqual(["openidm-query-filter"]);
      return;
    }
    expect(response.outcome, JSON.stringify(response.error)).toBe("ok");
    expect((parseHarvest(response.value as string).sharedState.final as Record<string, unknown>).r, why).toBe(JSON.stringify(answer));
  });
});
