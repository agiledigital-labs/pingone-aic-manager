import { beforeEach, describe, expect, it, vi } from "vitest";
import { conform } from "../../src/aic/conform.ts";
import { caseWith } from "./helpers.ts";
import { makeEffects } from "../case/helpers.ts";

const tenant = vi.hoisted(() => ({
  discover: vi.fn<() => Promise<string>>(),
  lane: vi.fn(),
}));

vi.mock("../../src/aic/run.ts", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../src/aic/run.ts")>()),
  discoverTenantCookieName: tenant.discover,
  runAicLane: tenant.lane,
}));

describe("conform with the tenant lane", () => {
  beforeEach(() => {
    tenant.discover.mockReset();
    tenant.lane.mockReset();
  });

  it("reports a failed cookie-name discovery on the AIC side and still runs the local lane", async () => {
    tenant.discover.mockRejectedValue(new Error("agent is locked"));
    const seen: Array<string | undefined> = [];
    const report = await conform({
      kase: caseWith({}),
      source: 'action.goTo("true");',
      local: async ({ kase }) => {
        seen.push(kase.given.cookieName);
        return makeEffects();
      },
      aic: "tenant",
    });
    expect(report.local.effects).toBeDefined();
    expect(report.local.verdict?.pass).toBe(true);
    expect(report.aic.error).toBe("tenant cookie-name discovery failed: agent is locked");
    expect(tenant.lane).not.toHaveBeenCalled();
    // No substitute name: the local lane runs without one.
    expect(seen).toEqual([undefined]);
  });

  it("seeds the discovered name into the local lane when discovery succeeds", async () => {
    tenant.discover.mockResolvedValue("sid");
    tenant.lane.mockResolvedValue(makeEffects());
    const seen: Array<string | undefined> = [];
    const report = await conform({
      kase: caseWith({}),
      source: 'action.goTo("true");',
      local: async ({ kase }) => {
        seen.push(kase.given.cookieName);
        return makeEffects();
      },
      aic: "tenant",
    });
    expect(seen).toEqual(["sid"]);
    expect(report.aic.error).toBeUndefined();
    expect(tenant.lane).toHaveBeenCalledOnce();
  });
});
