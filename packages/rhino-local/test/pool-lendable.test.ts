import { describe, expect, it } from "vitest";
import { lendable } from "../src/pool.ts";

const idle = { active: 0, retired: false, unshuttered: false };

describe("lendable", () => {
  it("lends an idle, unretired, always-shuttered, usable JVM", () => {
    expect(lendable(idle, true)).toBe(true);
  });

  it.each([
    ["no status answer", undefined, true],
    ["a job still running", { ...idle, active: 1 }, true],
    // The branch no e2e test can isolate: no shuttered job retires a JVM.
    ["retired while shuttered", { ...idle, retired: true }, true],
    ["ran unshuttered", { ...idle, unshuttered: true }, true],
    ["exited or retired on the process side", idle, false],
    ["missing active", { retired: false, unshuttered: false }, true],
    ["missing retired", { active: 0, unshuttered: false }, true],
    ["missing unshuttered", { active: 0, retired: false }, true],
  ] as const)("refuses %s", (_name, status, usable) => {
    expect(lendable(status, usable)).toBe(false);
  });
});
