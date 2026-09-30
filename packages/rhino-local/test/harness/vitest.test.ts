import { describe, expect, it } from "vitest";
import type { RhinoRunner } from "../../src/runner.ts";
import { aicWhenEnabled, defineSuite } from "../../src/harness/index.ts";
import {
  AIC_LANE_ENV,
  claimAicLeaseForFile,
  releaseAicLeaseForFile,
  useLease,
} from "../../src/harness/vitest.ts";
import type { UseLeaseOptions } from "../../src/harness/vitest.ts";

describe("useLease AIC file ownership", () => {
  it("does not expose identity fields that the adapter cannot honor", () => {
    const identityKeysAreAbsent: Extract<keyof UseLeaseOptions,
      "scriptName" | "loggerScriptId" | "oneShotRunId"> extends never ? true : false = true;
    expect(identityKeysAreAbsent).toBe(true);
  });

  it("refuses a local scriptName that cannot be uploaded under that name", () => {
    const suite = defineSuite({
      name: "named",
      scriptName: "deployed-name",
      script: 'action.goTo("done");',
      outcomes: ["done"],
    });
    expect(() => useLease(suite, { aic: { id: "named" } })).toThrow(
      /remove scriptName for AIC conformance/
    );
  });

  // Regression: aicWhenEnabled used to make this invalid only with AIC on.
  it("rejects explicit scriptName even when aicWhenEnabled is off", () => {
    const previous = process.env[AIC_LANE_ENV];
    const suite = defineSuite({
      name: "named",
      scriptName: "deployed-name",
      script: 'action.goTo("done");',
      outcomes: ["done"],
    });
    try {
      process.env[AIC_LANE_ENV] = "0";
      const off = aicWhenEnabled("named");
      expect(off.aic).toBeUndefined();
      expect(() => useLease(suite, off)).toThrow(/remove scriptName for AIC conformance/);
      process.env[AIC_LANE_ENV] = "1";
      expect(() => useLease(suite, aicWhenEnabled("named"))).toThrow(
        /remove scriptName for AIC conformance/
      );
    } finally {
      if (previous === undefined) {
        delete process.env[AIC_LANE_ENV];
      } else {
        process.env[AIC_LANE_ENV] = previous;
      }
    }
  });

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
