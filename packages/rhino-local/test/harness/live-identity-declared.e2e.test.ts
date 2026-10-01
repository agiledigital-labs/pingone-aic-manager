import { describe, expect, it } from "vitest";
import { AIC_LANE_ENV, aicWhenEnabled, defineSuite, managed, useLease } from "../../src/harness/index.ts";

const ID = "00000000-0000-4000-8000-0000000000de";
const RESOURCE = `managed/alpha_user/${ID}`;

// displayName has no measured layout, so the local lane refuses it unless
// the suite declares one. The declaration is local-only: the AIC lane writes
// through the tenant's real mapping, so a right declaration agrees and a
// wrong one disagrees — which is why it does not make a case AIC-ineligible.
const suite = defineSuite({
  name: "identity-declared",
  script: [
    'function t(f) { try { return String(f()); } catch (e) { return "threw:" + e; } }',
    `var id = "${ID}";`,
    `var path = "managed/alpha_user/${ID}";`,
    'function idm(f) { var v = openidm.read(path, null, [f])[f]; return v === null || v === undefined ? "absent" : JSON.stringify(v); }',
    '(function () { var w = idRepository.getIdentity(id); nodeState.putShared("store", t(function () { w.setAttribute("displayName", ["Shown"]); w.store(); return "ok"; })); })();',
    'nodeState.putShared("idmDisplayName", t(function () { return idm("displayName"); }));',
    'nodeState.putShared("am", t(function () { return JSON.stringify(idRepository.getIdentity(id).getAttributeValues("displayName").toArray().map(String)); }));',
    'action.goTo("done");',
  ].join("\n"),
  outcomes: ["done"],
  fixtures: {
    subject: managed("managed/alpha_user", {
      _id: ID, userName: "identity-declared-probe", mail: "identity-declared-probe@example.com",
      givenName: "Identity", sn: "Probe",
    }),
  },
  cleanup: async (idm) => {
    await idm.delete(RESOURCE);
  },
});

const WRITE = [{ identity: ID, attribute: "displayName", values: ["Shown"] }];

describe("declared identity attributes", () => {
  const lease = useLease(suite, aicWhenEnabled("live-identity-declared"));

  // Measured 2026-10-01 on alpha: AM's displayName lands in IDM displayName
  // as a string, matching this declaration on both lanes.
  it("agrees with the tenant when the declaration matches its mapping", async () => {
    const run = await lease.run()
      .identityAttributes({ displayName: { field: "displayName", cardinality: "single" } })
      .expect({
        outcome: "done",
        sharedState: { added: { store: "ok", idmDisplayName: '"Shown"', am: '["Shown"]' } },
        identityWrites: WRITE,
      });
    expect(run.verdict.pass).toBe(true);
  });

  it("is caught by the tenant lane when the declaration is wrong", async () => {
    const pending = lease.run()
      .identityAttributes({ displayName: { field: "description", cardinality: "single" } })
      .expect({
        outcome: "done",
        // What the wrong declaration makes the local lane believe.
        sharedState: { added: { store: "ok", idmDisplayName: "absent", am: '["Shown"]' } },
        identityWrites: WRITE,
      });
    if (process.env[AIC_LANE_ENV] !== "1") {
      expect((await pending).verdict.pass).toBe(true);
      return;
    }
    const error = await pending.then(() => undefined, (caught: unknown) => caught);
    expect(error).toBeInstanceOf(Error);
    const message = (error as Error).message;
    expect(message).toMatch(/^AIC verdict failed for /);
    expect(message).toContain('"idmDisplayName"="\\"Shown\\""');
    expect(message).not.toContain('"am"=');
  });
});
