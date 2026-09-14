import { describe, expect, it } from "vitest";
import { acquireManagedFixtureLock } from "../../src/aic/managed.ts";
import type { TenantSession } from "../../src/aic/tenant.ts";

const SESSION: TenantSession = {
  tenantName: "sandbox",
  baseUrl: "https://tenant.example.com",
  token: "test-token",
  project: "/tmp/rhino-local-aic-test",
};

describe("managed fixture isolation", () => {
  it("serializes two runs targeting the same tenant", async () => {
    const releaseFirst = await acquireManagedFixtureLock(SESSION, "first test");
    let firstReleased = false;
    let releaseSecond: (() => Promise<void>) | undefined;
    let secondAcquired = false;
    const second = acquireManagedFixtureLock(SESSION, "second test").then(
      (release) => {
        secondAcquired = true;
        return release;
      }
    );

    try {
      await new Promise((resolve) => setTimeout(resolve, 20));
      expect(secondAcquired).toBe(false);
      await releaseFirst();
      firstReleased = true;
      releaseSecond = await second;
      expect(secondAcquired).toBe(true);
    } finally {
      if (!firstReleased) {
        await releaseFirst();
      }
      releaseSecond ??= await second;
      await releaseSecond();
    }
  });
});
