import { beforeAll, describe, expect, it } from "vitest";
import { validateCase } from "../../../src/case/index.ts";

const widget = {
  name: "widget",
  script: "var x = 1;",
  expect: { outcome: "true" as const },
};

it("refuses a legacy engine", () => {
  validateCase({ ...widget, given: { engine: "legacy" } });
});

it("refuses an unknown key", () => {
  validateCase({ ...widget, extra: true });
});

it("catches a legacy refusal", () => {
  expect(() => validateCase({ ...widget, given: { engine: "legacy" } })).toThrow(/no longer emulated/);
});

it("accepts a next-gen case", () => {
  expect(validateCase({ ...widget, given: { engine: "next-gen" } }).given.engine).toBe("next-gen");
});

// A beforeAll refusal is stored on the suite. Its tests are skipped, so a
// reporter that only reads failed tests never counts it. Two suites, so one
// shared owner would under-count.
describe("nested setup", () => {
  describe("first hook", () => {
    beforeAll(() => {
      validateCase({ name: "first", script: "var x = 1;" });
    });
    it("one", () => {});
  });
  describe("second hook", () => {
    beforeAll(() => {
      validateCase({ name: "second", script: "var x = 1;" });
    });
    it("two", () => {});
  });
});
