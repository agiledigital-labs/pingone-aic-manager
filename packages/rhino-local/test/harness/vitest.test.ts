import { describe, expect, it } from "vitest";
import type { RhinoRunner } from "../../src/runner.ts";
import { defineSuite } from "../../src/harness/index.ts";
import {
  claimAicLeaseForFile,
  releaseAicLeaseForFile,
} from "../../src/harness/vitest.ts";

describe("useLease AIC file ownership", () => {
  it("rejects a second AIC lease in the same file even when its id differs", () => {
    const file = `/tmp/rhino-local-claim-${process.pid}-different-id.test.ts`;
    claimAicLeaseForFile(file, "first");
    try {
      expect(() => claimAicLeaseForFile(file, "second")).toThrow(
        /already has AIC lease "first"/
      );
    } finally {
      releaseAicLeaseForFile(file, "first");
    }
  });

  it("does not let a mismatched owner release another file lease", () => {
    const file = `/tmp/rhino-local-claim-${process.pid}-owner.test.ts`;
    claimAicLeaseForFile(file, "owner");
    releaseAicLeaseForFile(file, "stranger");
    try {
      expect(() => claimAicLeaseForFile(file, "next")).toThrow(
        /already has AIC lease "owner"/
      );
    } finally {
      releaseAicLeaseForFile(file, "owner");
    }
  });
});

describe("Lease.endTest", () => {
  it("clears local test fixtures even when lane cleanup fails", async () => {
    const suite = defineSuite({
      name: "cleanup failure",
      script: 'action.goTo("done");',
      outcomes: ["done"],
    });
    const lease = suite.lease({
      runner: {} as RhinoRunner,
      lane: {
        run: () => Promise.reject(new Error("unused")),
        endTest: () => Promise.reject(new Error("remote cleanup failed")),
      },
    });
    await lease.fixtures.create("managed/alpha_role", {
      _id: "test-only",
    });

    await expect(lease.endTest()).rejects.toThrow(/remote cleanup failed/);
    expect(lease.ledger()).toEqual([]);
  });
});
