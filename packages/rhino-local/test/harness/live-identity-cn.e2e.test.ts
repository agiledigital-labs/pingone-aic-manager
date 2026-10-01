import { describe, expect, it } from "vitest";
import { AIC_LANE_ENV, aicWhenEnabled, defineSuite, managed, useLease } from "../../src/harness/index.ts";

const ID = "00000000-0000-4000-8000-0000000000dd";
const RESOURCE = `managed/alpha_user/${ID}`;

// cn is the one measured attribute the local lane refuses to write: AM keeps
// it outside the IDM managed record, which is all this lane models. The lanes
// therefore disagree by design, and this file pins both sides exactly — the
// local refusal as the verdict, the tenant's values through the AIC lane's
// failure report.
const suite = defineSuite({
  name: "identity-cn",
  script: [
    'function t(f) { try { return String(f()); } catch (e) { return "threw:" + e; } }',
    `var id = "${ID}";`,
    `var path = "managed/alpha_user/${ID}";`,
    "function am() { return JSON.stringify(idRepository.getIdentity(id).getAttributeValues(\"cn\").toArray().map(String)); }",
    'function idmHasCn() { return String(Object.prototype.hasOwnProperty.call(openidm.read(path), "cn")); }',
    'nodeState.putShared("cnBefore", t(am));',
    'nodeState.putShared("idmHasCnBefore", t(idmHasCn));',
    '(function () { var w = idRepository.getIdentity(id); nodeState.putShared("cnStore", t(function () { w.setAttribute("cn", ["Common Name"]); w.store(); return "ok"; })); })();',
    'nodeState.putShared("cnAfter", t(am));',
    'nodeState.putShared("idmHasCnAfter", t(idmHasCn));',
    '(function () { var w = idRepository.getIdentity(id); nodeState.putShared("cnEmptyStore", t(function () { w.setAttribute("cn", []); w.store(); return "ok"; })); })();',
    'nodeState.putShared("cnAfterEmpty", t(am));',
    'action.goTo("done");',
  ].join("\n"),
  outcomes: ["done"],
  fixtures: {
    subject: managed("managed/alpha_user", {
      _id: ID, userName: "identity-cn-probe", mail: "identity-cn-probe@example.com",
      givenName: "Identity", sn: "Probe",
    }),
  },
  cleanup: async (idm) => {
    await idm.delete(RESOURCE);
  },
});

const REFUSAL =
  'threw:Error: rhino-local: idRepository.getIdentity().setAttribute: AM stores "cn" outside the IDM managed record (measured), which the local lane does not model, so it refuses the write';

// Measured 2026-10-01 on alpha: cn starts as "<givenName> <sn>", store()
// changes it and AM reads the new value back, IDM never has a cn property,
// and an empty write is refused (cn is required).
const TENANT = [
  'added "cnBefore"="[\\"Identity Probe\\"]"',
  'added "cnStore"="ok"',
  'added "cnAfter"="[\\"Common Name\\"]"',
  'added "cnEmptyStore"="threw:JavaException: org.forgerock.openam.scripting.api.identity.ScriptedIdentityScriptWrapper$IdentityUpdateException: Exception persisting attribute: Plug-in org.forgerock.openam.idrepo.ldap.DJLDAPv3Repo encountered a ldap exception.  ldap errorcode=65"',
  'added "cnAfterEmpty"="[\\"Common Name\\"]"',
];

describe("identity cn", () => {
  const lease = useLease(suite, aicWhenEnabled("live-identity-cn"));

  it("refuses a cn write locally; the tenant stores it outside IDM", async () => {
    const pending = lease.run().expect({
      outcome: "done",
      sharedState: { added: {
        cnBefore: "[]",
        idmHasCnBefore: "false",
        cnStore: REFUSAL,
        cnAfter: "[]",
        idmHasCnAfter: "false",
        cnEmptyStore: REFUSAL,
        cnAfterEmpty: "[]",
      } },
      identityWrites: [],
    });
    if (process.env[AIC_LANE_ENV] !== "1") {
      expect((await pending).verdict.pass).toBe(true);
      return;
    }
    const error = await pending.then(() => undefined, (caught: unknown) => caught);
    expect(error).toBeInstanceOf(Error);
    const message = (error as Error).message;
    expect(message).toMatch(/^AIC verdict failed for /);
    for (const line of TENANT) {
      expect(message).toContain(line);
    }
    // idmHasCn* agree with the local expectation, so they are not reported.
    expect(message).not.toContain("idmHasCn");
  });
});
