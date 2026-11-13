/**
 * A case-validation refusal is a named fault. The thrown message stays the
 * detailed one; the id is one per kind. Two different unknown keys share an id.
 */
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import type { RhinoRunner } from "../../src/runner.ts";
import { runCase } from "../../src/bindings/run.ts";
import { CaseValidationError, validateCase } from "../../src/case/index.ts";
import { CASE_VALIDATION_FAULTS } from "../../src/case/validation-fault.ts";
import { tallyFaults } from "../../src/harness/fault-tally.ts";
import { caseValidationAudit } from "../../src/harness/validation-audit.ts";
import { TIMING_ENV, summarizeTiming } from "../../src/timing.ts";

function refuse(input: unknown): CaseValidationError {
  try {
    validateCase(input);
  } catch (error) {
    expect(error).toBeInstanceOf(CaseValidationError);
    return error as CaseValidationError;
  }
  throw new Error("validateCase accepted a case it should refuse");
}

const base = { name: "widget", script: "var x = 1;", expect: { outcome: "true" } };

describe("case validation fault ids", () => {
  it("gives the legacy engine its own id and keeps the detailed message", () => {
    const error = refuse({ ...base, given: { engine: "legacy" } });
    expect(error.faultId).toBe("case-invalid/engine-legacy");
    expect(error.name).toBe("CaseValidationError/case-invalid/engine-legacy");
    expect(error.message).toMatch(/case\.given\.engine "legacy": the legacy \(V1\) script engine is no longer emulated/);
    expect(CASE_VALIDATION_FAULTS[error.faultId]).not.toContain("widget");
  });

  it.each([
    ["another engine", { ...base, given: { engine: "rhino" } }, "case-invalid/engine"],
    ["not an object", null, "case-invalid/not-object"],
    ["an unknown key", { ...base, extra: 1 }, "case-invalid/unknown-key"],
    ["a different unknown key", { ...base, other: 1 }, "case-invalid/unknown-key"],
    ["a missing expect", { name: "widget", script: "var x = 1;" }, "case-invalid/required"],
    ["an empty outcome list", { ...base, outcomes: [] }, "case-invalid/empty"],
    ["a repeated outcome", { ...base, outcomes: ["true", "true"] }, "case-invalid/duplicate"],
    ["a context that decides no outcome", { ...base, context: "OIDC_CLAIMS", outcomes: ["x"], expect: { outcome: null } }, "case-invalid/context"],
    ["two seeds of one attribute", { ...base, given: { registeredObjectAttributes: { probe: "old" }, sharedState: { objectAttributes: {} } } }, "case-invalid/conflict"],
    ["a value of the wrong type", { ...base, outcomes: "true" }, "case-invalid/wrong-type"],
  ] as const)("%s", (_label, input, id) => {
    expect(refuse(input).faultId).toBe(id);
  });

  it("counts a failed validation as a tally row and ignores a refusal the test caught", () => {
    const audit = caseValidationAudit([
      {
        file: "/proj/test/a.test.ts",
        ownerId: "failed",
        ownerName: "refuses legacy",
        errors: [{ name: "CaseValidationError/case-invalid/engine-legacy" }],
      },
      {
        file: "/proj/test/a.test.ts",
        ownerId: "other",
        ownerName: "unknown key",
        errors: [{ name: "Error", cause: { name: "CaseValidationError/case-invalid/unknown-key" } }],
      },
    ]);
    const tally = tallyFaults(audit === undefined ? [] : [audit], "/proj");
    expect(tally.faults).toEqual([
      {
        id: "case-invalid/engine-legacy",
        cases: 1,
        failed: 1,
        files: ["test/a.test.ts"],
        message: CASE_VALIDATION_FAULTS["case-invalid/engine-legacy"],
      },
      {
        id: "case-invalid/unknown-key",
        cases: 1,
        failed: 1,
        files: ["test/a.test.ts"],
        message: CASE_VALIDATION_FAULTS["case-invalid/unknown-key"],
      },
    ]);
    expect(JSON.stringify(tally)).not.toContain("widget");
  });

  it("records the refusal on a timing row whose --share --json output keeps the id and drops the name", async () => {
    const dir = mkdtempSync(join(tmpdir(), "rl-validation-fault-"));
    const file = join(dir, "timing.jsonl");
    const previous = process.env[TIMING_ENV];
    process.env[TIMING_ENV] = file;
    const runner = { eval(): never { throw new Error("the JVM should not run"); } } as unknown as RhinoRunner;
    try {
      await expect(runCase(runner, { ...base, name: "secret-case", given: { engine: "legacy" as never } })).rejects.toBeInstanceOf(CaseValidationError);
    } finally {
      if (previous === undefined) delete process.env[TIMING_ENV];
      else process.env[TIMING_ENV] = previous;
    }
    const summary = summarizeTiming(readFileSync(file, "utf8"), { share: { salt: "s" } });
    rmSync(dir, { recursive: true, force: true });
    expect(summary.faults).toEqual([{ id: "case-invalid/engine-legacy", jobs: 1 }]);
    const shared = `${JSON.stringify(summary)}\n`;
    expect(shared).toContain("case-invalid/engine-legacy");
    expect(shared).not.toContain("secret-case");
    // Validation never reaches a JVM, so there is no jvm.totalMs to bucket.
    // Share therefore emits no source row at all; the case name is absent, not rewritten.
    expect(summary.sources).toEqual([]);
  });
});
