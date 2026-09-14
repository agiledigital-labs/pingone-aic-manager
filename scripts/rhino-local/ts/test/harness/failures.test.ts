import { mkdtemp, readFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import {
  appendFailure,
  failureRecordFor,
  parseFailures,
  readFailures,
  recordFailureIfAny,
  sortNewestFirst,
  type FailureRecord,
} from "../../src/harness/failures.ts";

const BASE = {
  testName: "suite > the test",
  file: "test/harness/lease.e2e.test.ts",
  suite: "resolve-identity",
  timestamp: "2026-09-14T12:00:00.000Z",
} as const;

describe("failureRecordFor", () => {
  it("round-trips a real AIC trace into a record", () => {
    const record = failureRecordFor({
      ...BASE,
      trace: {
        stem: "stem-a",
        passIds: ["stem-a-01", "stem-a-02"],
        tenantName: "sandbox",
      },
    });
    expect(record).toEqual({
      ...BASE,
      stem: "stem-a",
      passIds: ["stem-a-01", "stem-a-02"],
      tenant: "sandbox",
    });
  });

  it("a local-only failure does not produce a record that would fetch nothing", () => {
    expect(failureRecordFor({ ...BASE, trace: undefined })).toBeUndefined();
    expect(
      failureRecordFor({
        ...BASE,
        trace: { stem: "", passIds: [], tenantName: "sandbox" },
      })
    ).toBeUndefined();
    expect(
      failureRecordFor({
        ...BASE,
        trace: { stem: "stem-a", passIds: [], tenantName: "sandbox" },
      })
    ).toBeUndefined();
  });
});

describe("failure JSONL", () => {
  it("round-trips a record through the dump file", async () => {
    const path = join(await mkdtemp(join(tmpdir(), "rl-fail-")), "failures.jsonl");
    const record = sample("one");
    await appendFailure(record, path);
    expect(await readFailures(path)).toEqual([record]);
    expect(await readFile(path, "utf8")).toBe(`${JSON.stringify(record)}\n`);
  });

  it("two concurrent writers both survive", async () => {
    const path = join(await mkdtemp(join(tmpdir(), "rl-fail-")), "failures.jsonl");
    const a = sample("a");
    const b = sample("b");
    await Promise.all([appendFailure(a, path), appendFailure(b, path)]);
    const read = await readFailures(path);
    expect(read).toHaveLength(2);
    expect(read.map((row) => row.testName).sort()).toEqual(["a", "b"]);
  });

  it("recordFailureIfAny does not create a file for a local-only failure", async () => {
    const path = join(await mkdtemp(join(tmpdir(), "rl-fail-")), "failures.jsonl");
    const written = await recordFailureIfAny({ ...BASE, trace: undefined }, path);
    expect(written).toBeUndefined();
    expect(await readFailures(path)).toEqual([]);
  });

  it("skips a malformed line rather than refusing the rest of the dump", () => {
    const good = sample("good");
    const text = `not json\n${JSON.stringify(good)}\n{"stem":"x"}\n`;
    expect(parseFailures(text)).toEqual([good]);
  });

  it("sorts newest first, and later-equal timestamps win", () => {
    const older = sample("older", "2026-09-14T10:00:00.000Z");
    const newer = sample("newer", "2026-09-14T12:00:00.000Z");
    const sameA = sample("same-a", "2026-09-14T11:00:00.000Z");
    const sameB = sample("same-b", "2026-09-14T11:00:00.000Z");
    expect(sortNewestFirst([older, sameA, sameB, newer]).map((row) => row.testName)).toEqual([
      "newer",
      "same-b",
      "same-a",
      "older",
    ]);
  });
});

function sample(testName: string, timestamp: string = BASE.timestamp): FailureRecord {
  return {
    testName,
    stem: `stem-${testName}`,
    passIds: [`stem-${testName}-01`],
    file: BASE.file,
    suite: BASE.suite,
    timestamp,
    tenant: "sandbox",
  };
}
