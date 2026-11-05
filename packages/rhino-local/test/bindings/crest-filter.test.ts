/** The shared `_queryFilter` table through the grammar and the script tester's openidm.query. */
import { createRequire } from "node:module";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { parseHarvest } from "../../src/bindings/harvest.ts";
import { mockPreamble, withHarvest } from "../../src/bindings/preamble.ts";
import { expectHarnessFault } from "../../src/harness/emulation-guard.ts";
import { bindingsCrestFilterPath } from "../../src/paths.ts";
import { RhinoRunner } from "../../src/runner.ts";
import { type Answer, filterTable, records } from "../crest-filter-table.ts";
import type { JsonObject } from "../../src/case/types.ts";
import { loadBehaviour, sandboxFaults } from "./load-behaviour.ts";

const grammar = createRequire(import.meta.url)(bindingsCrestFilterPath) as {
  parse(input: unknown): { ast?: unknown; error?: string };
};
const given = { managed: { "managed/widget": records.map((record) => structuredClone(record) as JsonObject) } };
const parseText = (filter: unknown) =>
  `rhino-local: openidm.query: The value '${String(filter)}' for parameter '_queryFilter' could not be parsed as a valid query filter`;
const rows = (answer: Answer) => (Array.isArray(answer) ? answer : []);

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
});
