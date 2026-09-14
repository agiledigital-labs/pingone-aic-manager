import { describe, expect, it } from "vitest";
import {
  claimAicLeaseForFile,
  releaseAicLeaseForFile,
} from "../../src/harness/index.ts";

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
