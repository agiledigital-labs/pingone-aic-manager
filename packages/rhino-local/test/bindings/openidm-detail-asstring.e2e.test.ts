/**
 * `asString()`, `get(i)` and member presence on an injected openidm failure's
 * `getDetail()` JsonValue, on the real JVM, compared with the live capture
 * (openidm-detail-asstring probe, sandbox, 2026-11-10; docs/api/12). The
 * script is the probe fixture itself, verbatim, with only its `emit`
 * replaced; its two calls are failed by stubs carrying the measured details.
 */
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { parseHarvest } from "../../src/bindings/harvest.ts";
import { mockPreamble, withHarvest } from "../../src/bindings/preamble.ts";
import type { Given } from "../../src/case/types.ts";
import { repoRoot } from "../../src/paths.ts";
import { RhinoRunner } from "../../src/runner.ts";

const probe = readFileSync(join(repoRoot, "scripts/rhino-script-tester/fixtures/openidm-detail-asstring.script.js"), "utf8");
// The probe names its throwaway user with randomUUID; pin it so the host and
// container lanes see the same id and the both-lane comparison stays exact.
const pinUuid = 'utils.crypto.randomUUID = function () { return "00000000-0000-4000-8000-000000000001"; };\n';
const capture = `${pinUuid}${probe}\nfunction emit(payload) { nodeState.putShared("r", JSON.stringify(payload)); }`;

const POLICY = { result: false, failedPolicyRequirements: [{ policyRequirements: [{ policyRequirement: "NOT_NULL" }], property: "frIndexedInteger1" }] };
const given: Given = {
  managed: { "managed/bravo_user": [], "managed/alpha_user": [] },
  openidmFailures: [
    { match: { method: "create", resource: /^managed\/bravo_user\//, ordinal: 1 }, reply: { code: 403, message: "Policy validation failed", detail: POLICY } },
    { match: { method: "delete", resource: /^managed\/alpha_user\//, ordinal: 1 }, reply: { code: 404, message: "No Such Entry", detail: null } },
  ],
};

// The capture's error strings end in " (<script>#<line>)", which names the probe's own script.
const located = (value: unknown): unknown =>
  typeof value === "string" ? value.replace(/ \([^()]*#\d+\)$/, "")
    : Array.isArray(value) ? value.map(located)
      : value !== null && typeof value === "object" ? Object.fromEntries(Object.entries(value).map(([k, v]) => [k, located(v)])) : value;

const str = (value: string) => ({ typeof: "string", isNull: false, value, eq: value === "frIndexedInteger1", length: value.length });
const nul = { typeof: "object", isNull: true, value: null, eq: false, length: null };
const nullValue = { typeof: "object", isNullJs: false, isNull: true, str: "null" };
const expecting = (pointer: string, cls: string) => `err:InternalError: ${pointer}: Expecting a java.lang.String but got ${cls}`;
const fn = { typeof: "function", in: true };
const presence = {
  asString: fn, isString: fn, asBoolean: fn, asInteger: fn, asList: fn, asMap: fn, isNull: fn, get: fn, size: fn, keys: fn, expect: fn, defaultTo: fn, required: fn,
  class: { typeof: 'err:InternalError: Access to Java class "java.lang.Class" is prohibited.', in: true },
  zzNotAMember: { typeof: "undefined", in: false },
};
const FPR = "/detail/failedPolicyRequirements";

describe("a failure detail's asString, get(i) and member presence match the live capture", () => {
  let runner: RhinoRunner;
  beforeAll(async () => { runner = await RhinoRunner.spawn(); }, 60_000);
  afterAll(async () => { await runner?.close(); }, 30_000);

  it("policy 403 object detail and 404 null detail", async () => {
    const response = await runner.eval({ preamble: mockPreamble(given), source: withHarvest(capture) });
    expect(response.outcome, JSON.stringify(response.error)).toBe("ok");
    expect(response.emulation?.faults ?? []).toEqual([]);
    const out = JSON.parse(parseHarvest(response.value as string).sharedState.final.r as string) as Record<string, unknown>;
    expect(located(out)).toEqual({
      ok: true,
      feature: "openidm-detail-asstring",
      policyNull: false,
      notFoundNull: false,
      getIndex: { typeof: "object", str: '{ "policyRequirements": [ { "policyRequirement": "NOT_NULL" } ], "property": "frIndexedInteger1" }' },
      property: { typeof: "object", str: '"frIndexedInteger1"', isNull: false },
      asStringOnString: str("frIndexedInteger1"),
      asStringOnNullDetail: nul,
      asStringOnNullFromNullDetail: nul,
      missing: nullValue,
      asStringOnMissing: nul,
      asStringOnBoolean: expecting("/detail/result", "java.lang.Boolean"),
      asStringOnObject: expecting("/detail", "java.util.LinkedHashMap"),
      asStringOnArray: expecting(FPR, "java.util.ArrayList"),
      asStringWithArg: "err:InternalError: Can't find method org.forgerock.json.JsonValue.asString(number).",
      asStringValue: "function asString() {/*\njava.lang.String asString()\n*/}\n",
      asStringOnElement: expecting(`${FPR}/0`, "java.util.LinkedHashMap"),
      asStringDeepArray: expecting(`${FPR}/0/policyRequirements`, "java.util.ArrayList"),
      asStringDeepString: str("NOT_NULL"),
      getOutOfRange: nullValue,
      getNegative: `err:InternalError: ${FPR}: List index out of range: -1`,
      getKeyOnArray: nullValue,
      getIndexOnObject: nullValue,
      getIndexOnString: { typeof: "object", str: "null" },
      getKeyOnNull: { typeof: "object", isNull: true },
      getIndexOnNull: { typeof: "object", isNull: true },
      presencePolicy: presence,
      presenceNull: presence,
      presenceString: presence,
    });
  });
});
