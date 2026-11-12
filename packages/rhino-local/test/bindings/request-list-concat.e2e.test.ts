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

  it("a size-zero seed is an empty list, and a one-element list of an empty string is not", async () => {
    const response = await runner.eval({
      preamble: mockPreamble({ requestParameters: { q: [], e: [""] } } as Given),
      source: withHarvest('var e = requestParameters.get("e"); nodeState.putShared("r", requestParameters.get("q").size() + "|" + JSON.stringify([].concat(e)) + "|" + String(e) + "|" + e.length + "|" + JSON.stringify(Array.prototype.slice.call(e)));'),
    });
    expect(response.outcome, JSON.stringify(response.error)).toBe("ok");
    expect(response.emulation?.faults ?? []).toEqual([]);
    expect(parseHarvest(response.value as string).sharedState.final.r).toBe('0|[""]|[]|1|[""]');
  });
});
