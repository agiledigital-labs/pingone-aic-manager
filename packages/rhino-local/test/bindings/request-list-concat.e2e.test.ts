/**
 * `[].concat`, `String` and `JSON.stringify` on the value lists
 * requestParameters / requestHeaders hand out, on the real JVM, compared with
 * the live capture (request-list-concat probe, sandbox, 2026-11-10;
 * docs/api/12). The script is the probe fixture itself, verbatim, with only
 * its `emit` replaced so the payload lands in shared state. The capture was
 * identical for parameters and headers, so one expectation serves both.
 */
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { mockPreamble, withHarvest } from "../../src/bindings/preamble.ts";
import { parseHarvest } from "../../src/bindings/harvest.ts";
import type { Given } from "../../src/case/types.ts";
import { expectHarnessFault } from "../../src/harness/emulation-guard.ts";
import { repoRoot } from "../../src/paths.ts";
import { RhinoRunner } from "../../src/runner.ts";

const probe = readFileSync(join(repoRoot, "scripts/rhino-script-tester/fixtures/request-list-concat.script.js"), "utf8");
const capture = `${probe}\nfunction emit(payload) { nodeState.putShared("r", JSON.stringify(payload)); }`;

// The request the live run sent: each probe header and parameter twice, once,
// once comma-joined, and once empty (`probeqempty=`, `X-Aic-Probe-Empty;`).
const given = {
  requestParameters: { probeq: ["alpha", "bravo"], probeqsingle: ["solo"], probeqjoined: ["alpha,bravo"], probeqempty: [""] },
  requestHeaders: { "X-Aic-Probe": ["alpha", "bravo"], "X-Aic-Probe-Single": ["solo"], "X-Aic-Probe-Joined": ["alpha,bravo"], "X-Aic-Probe-Empty": [""] },
};

// The capture's record of a concat result holding `items`.
const result = (items: Array<string | number | null>) => ({
  typeof: "object",
  isArray: true,
  length: items.length,
  items: items.map((v) => ({ typeof: v === null ? "object" : typeof v, value: v === null ? null : String(v), isList: false })),
  eqAlpha: items[0] === "alpha",
  join: items.map((v) => (v === null ? "" : String(v))).join("|"),
  str: items.map((v) => (v === null ? "" : String(v))).join(","),
  json: JSON.stringify(items),
});
const AB = '["alpha","bravo"]';
const side = {
  got: { many: { typeof: "object", isNull: false, size: 2 }, empty: { typeof: "object", isNull: false, size: 1, str: "[]" }, missing: { typeof: "object", isNull: true } },
  concatMany: result(["alpha", "bravo"]),
  concatSingle: result(["solo"]),
  concatJoined: result(["alpha,bravo"]),
  concatEmpty: result([""]),
  concatMissing: result([null]),
  concatTwo: result(["alpha", "bravo", "solo"]),
  concatMixed: result(["x", "alpha", "bravo", 1, "solo", "y"]),
  concatOntoArray: result(["first", "alpha", "bravo"]),
  concatCallOnList: result(["alpha", "bravo", "z"]),
  typeofConcatMember: "undefined",
  strMany: "[alpha, bravo]",
  strEmpty: "[]",
  jsonMany: AB,
  jsonSingle: '["solo"]',
  jsonEmpty: '[""]',
  jsonNested: `{"k":${AB}}`,
  jsonInArray: `[${AB}]`,
  jsonFromToJSON: AB,
  jsonFromGetter: `{"v":${AB}}`,
  jsonFromReplacer: `{"v":${AB}}`,
  jsonReplacerSees: { out: `{"v":${AB}}`, seen: [false, "function", true] },
  jsonListedDirect: AB,
  jsonListedNested: `{"v":${AB}}`,
  jsonListedGetter: `{"v":${AB}}`,
  jsonIndented: '{\n "v": [\n  "alpha",\n  "bravo"\n ]\n}',
};

describe("request value lists under concat, String and JSON.stringify match the live capture", () => {
  let runner: RhinoRunner;
  beforeAll(async () => { runner = await RhinoRunner.spawn(); }, 60_000);
  afterAll(async () => { await runner?.close(); }, 30_000);

  it("parameters, headers and the two together", async () => {
    const response = await runner.eval({ preamble: mockPreamble(given), source: withHarvest(capture) });
    expect(response.outcome, JSON.stringify(response.error)).toBe("ok");
    expect(response.emulation?.faults ?? []).toEqual([]);
    const out = JSON.parse(parseHarvest(response.value as string).sharedState.final.r as string) as Record<string, unknown>;
    expect(out).toEqual({
      ok: true,
      feature: "request-list-concat",
      parameters: side,
      headers: side,
      cross: result(["alpha", "bravo", "alpha", "bravo"]),
    });
  });

  // Still unmeasured: an openidm list as concat's receiver (the request list's receiver form is measured above).
  it("an openidm list as concat's receiver still faults openidm-list-unmeasured", async () => {
    expectHarnessFault("openidm-list-unmeasured");
    const response = await runner.eval({
      preamble: mockPreamble({ managed: { "managed/alpha_user": [{ _id: "u1", _rev: "1" }] } } as Given),
      source: withHarvest('var v = openidm.query("managed/alpha_user", { _queryFilter: "true" }, ["_id"]).result; try { Array.prototype.concat.call(v, requestParameters.get("q")); } catch (e) {} finally { 1; }'),
    });
    expect(response.emulation?.faults?.map((f) => f.id)).toEqual(["openidm-list-unmeasured"]);
  });

  // A size-zero request list was never produced live (an empty value arrives as [""]), so a `[]` seed
  // is accepted but copying it is refused rather than answered with an assumed [].
  it.each([
    ["JSON.stringify of a parameter list", "JSON.stringify(requestParameters.get(\"q\"))"],
    ["JSON.stringify nested", "JSON.stringify({ v: requestParameters.get(\"q\") })"],
    ["[].concat of a parameter list", "[].concat(requestParameters.get(\"q\"))"],
    ["JSON.stringify of [].concat", "JSON.stringify([].concat(requestParameters.get(\"q\")))"],
    ["a parameter list as concat's receiver", "Array.prototype.concat.call(requestParameters.get(\"q\"), [1])"],
    ["[].concat of a header list", "[].concat(requestHeaders.get(\"x-zero\"))"],
    ["JSON.stringify under a property list", "JSON.stringify({ v: requestHeaders.get(\"x-zero\") }, [\"v\"])"],
  ])("%s of a size-zero list faults request-list-unmeasured, even when caught", async (_name, read) => {
    expectHarnessFault("request-list-unmeasured");
    const response = await runner.eval({
      preamble: mockPreamble({ requestParameters: { q: [] }, requestHeaders: { "X-Zero": [] } } as Given),
      source: withHarvest(`try { ${read}; } catch (e) {}`),
    });
    expect(response.emulation?.faults?.map((f) => f.id)).toEqual(["request-list-unmeasured"]);
  });

  it("size() of a size-zero seed is still answered, and a one-element list still copies", async () => {
    const response = await runner.eval({
      preamble: mockPreamble({ requestParameters: { q: [], e: [""] } } as Given),
      source: withHarvest('nodeState.putShared("r", requestParameters.get("q").size() + "|" + JSON.stringify([].concat(requestParameters.get("e"))));'),
    });
    expect(response.outcome, JSON.stringify(response.error)).toBe("ok");
    expect(response.emulation?.faults ?? []).toEqual([]);
    expect(parseHarvest(response.value as string).sharedState.final.r).toBe('0|[""]');
  });
});
