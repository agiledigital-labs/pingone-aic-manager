import { expect, it } from "vitest";
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
