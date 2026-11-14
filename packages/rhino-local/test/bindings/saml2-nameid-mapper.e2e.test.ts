/**
 * The SAML2 NameID mapper on the real JVM. These cases pin the probe results
 * from an SP-initiated SSO on 2026-11-14 (docs/api/12, "SAML2 NameID mapper"):
 * a string completion is the assertion NameID, an empty completion fails SSO
 * with "Could not generate NameID", and a throw fails SSO with the thrown
 * string in the exception text.
 */
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { CaseRunError, runCase } from "../../src/bindings/run.ts";
import { defineCase } from "../../src/case/validate.ts";
import { expectHarnessFault } from "../../src/harness/emulation-guard.ts";
import { RhinoRunner } from "../../src/runner.ts";

const EMAIL = "urn:oasis:names:tc:SAML:1.1:nameid-format:emailAddress";
const REMOTE = "http://localhost:18080/realms/zzemuR3S";
const HOSTED = "https://zzemuR3S-idp.example.com";
const MAIL = "zzemuR3S@example.com";
const SCRIPT_ID = "11111111-1111-4111-8111-111111111111";
const SCRIPT_NAME = "zzemuR3S-nameid";
const LOGGER = `scripts.SAML2_NAMEID_MAPPER.${SCRIPT_ID}.(${SCRIPT_NAME})`;

const given = {
  realm: "/alpha",
  scriptName: SCRIPT_NAME,
  loggerScriptId: SCRIPT_ID,
  identity: { name: "subject-id", exists: true, attributes: { mail: [MAIL] as [string] } },
  nameId: { format: EMAIL, remoteEntityId: REMOTE, hostedEntityId: HOSTED },
};

const SURFACE = [
  'var mail = identity.getAttributeValues("mail");',
  'logger.info("zzemuR3S-ran");',
  "[",
  "  typeof nameIDScriptHelper,",
  "  typeof identity,",
  "  nameIDFormat,",
  "  remoteEntityId,",
  "  hostedEntityId,",
  "  typeof session,",
  '  String(nameIDScriptHelper.getNameIDFromSession()),',
  "  nameIDScriptHelper.getNameIDValue(),",
  "  String(nameIDScriptHelper.shouldPersistNameIDFormat()),",
  "  String(identity.exists()),",
  "  identity.getName(),",
  "  String(mail.length),",
  `  mail[0] === ${JSON.stringify(MAIL)} ? "js-string" : "not-js-string",`,
  "  logger.getName()",
  '].join("|");',
].join("\n");

const EXPECTED = [
  "object",
  "object",
  EMAIL,
  REMOTE,
  HOSTED,
  "object",
  "null",
  MAIL,
  "false",
  "true",
  "subject-id",
  "1",
  "js-string",
  LOGGER,
].join("|");

describe("SAML2 NameID mapper matches the live SSO", () => {
  let runner: RhinoRunner;
  beforeAll(async () => { runner = await RhinoRunner.spawn(); }, 60_000);
  afterAll(async () => { await runner?.close(); }, 30_000);

  it("the completion string is the NameID, and the measured bindings answer", async () => {
    const { effects, verdict } = await runCase(runner, defineCase({
      name: "enumerate",
      context: "SAML2_NAMEID_MAPPER",
      script: SURFACE,
      given,
      expect: {
        nameId: { value: EXPECTED, format: EMAIL },
        logs: [{ level: "info", message: "zzemuR3S-ran" }],
      },
    }));
    expect(verdict.pass, verdict.summary).toBe(true);
    expect(effects.nameId).toEqual({ accepted: true, value: EXPECTED, format: EMAIL, refusal: null });
  }, 60_000);

  it("a completion of undefined fails with the measured refusal", async () => {
    const { effects, verdict } = await runCase(runner, defineCase({
      name: "void",
      context: "SAML2_NAMEID_MAPPER",
      script: 'logger.info("zzemuR3S-ran-void");',
      given,
      expect: {
        nameId: { accepted: false, refusal: "Could not generate NameID", format: EMAIL },
        logs: [{ level: "info", message: "zzemuR3S-ran-void" }],
      },
    }));
    expect(verdict.pass, verdict.summary).toBe(true);
    expect(effects.nameId?.accepted).toBe(false);
  }, 60_000);

  it("a thrown string fails the run and the message contains it", async () => {
    const error = await runCase(runner, defineCase({
      name: "throw",
      context: "SAML2_NAMEID_MAPPER",
      script: 'logger.info("zzemuR3S-ran-throw"); throw "zzemuR3S-boom";',
      given,
      expect: { nameId: { value: "unused" } },
    })).then(() => null, (caught: unknown) => caught);
    expect(error).toBeInstanceOf(CaseRunError);
    expect((error as CaseRunError).message).toContain("zzemuR3S-boom");
  }, 60_000);

  it.each(["1;", '"";'])("an unmeasured completion faults: %s", async (script) => {
    expectHarnessFault("saml2-nameid-mapper-unmeasured/result");
    const error = await runCase(runner, defineCase({
      name: "number",
      context: "SAML2_NAMEID_MAPPER",
      script,
      given,
      expect: { nameId: { value: "unused" } },
    })).then(() => null, (caught: unknown) => caught);
    expect(error).toBeInstanceOf(CaseRunError);
  }, 60_000);

  it.each([
    ['typeof nameIDScriptHelper.setNameIDValue; "ok";', "nameIDScriptHelper.setNameIDValue"],
    ['typeof identity.getAttributes; "ok";', "identity.getAttributes"],
    ['try { typeof nameIDScriptHelper.setNameIDValue; } catch (e) {} "ok";', "nameIDScriptHelper.setNameIDValue"],
    ['try { typeof identity.getAttributes; } catch (e) {} "ok";', "identity.getAttributes"],
    ['nameIDFormat = "changed"; "ok";', "nameIDFormat"],
    ['try { nameIDFormat = "changed"; } catch (e) {} "ok";', "nameIDFormat"],
    [`nameIDFormat = "${EMAIL}"; "ok";`, "nameIDFormat"],
  ])("unmeasured read or assignment records a fault: %s", async (script, member) => {
    expectHarnessFault(`saml2-nameid-mapper-unmeasured/${member}`);
    await expect(runCase(runner, defineCase({
      name: member, context: "SAML2_NAMEID_MAPPER", script, given,
      expect: { nameId: { value: "ok" } },
    }))).rejects.toBeInstanceOf(CaseRunError);
  }, 60_000);

  it.each(["getNameIDFromSession", "shouldPersistNameIDFormat"])(
    "refuses %s outside the measured email format", async (method) => {
      expectHarnessFault(`saml2-nameid-mapper-unmeasured/nameIDScriptHelper.${method}()`);
      // The review's combined expression must fault whichever operation runs first.
      const script = method === "shouldPersistNameIDFormat"
        ? 'String(nameIDScriptHelper.shouldPersistNameIDFormat()) + "|" + String(nameIDScriptHelper.getNameIDFromSession());'
        : 'String(nameIDScriptHelper.getNameIDFromSession()) + "|" + String(nameIDScriptHelper.shouldPersistNameIDFormat());';
      await expect(runCase(runner, defineCase({
        name: method, context: "SAML2_NAMEID_MAPPER", script,
        given: { ...given, nameId: { format: "urn:oasis:names:tc:SAML:2.0:nameid-format:persistent" } },
        expect: { nameId: { value: method === "shouldPersistNameIDFormat" ? "false|null" : "null|false" } },
      }))).rejects.toBeInstanceOf(CaseRunError);
    }, 60_000,
  );

  it("keeps every helper member measured as undefined", async () => {
    const names = ["class", "equals", "getClass", "hashCode", "notify", "notifyAll", "toString", "wait",
      "nameIDFromSession", "nameIDValue"];
    const { verdict } = await runCase(runner, defineCase({
      name: "undefined helper members", context: "SAML2_NAMEID_MAPPER", given,
      script: names.map((name) => `if (nameIDScriptHelper[${JSON.stringify(name)}] !== undefined) throw "unexpected";`).join("\n") + '\n"ok";',
      expect: { nameId: { value: "ok" } },
    }));
    expect(verdict.pass, verdict.summary).toBe(true);
  }, 60_000);

  it("createNameIdentifier, a format constant, another attribute and a wrong arity fault", async () => {
    const cases: [string, string][] = [
      ['session.getProperty("x");', "saml2-nameid-mapper-unmeasured/session.getProperty"],
      ['try { session.anyMember; } catch (e) {} "caught";', "saml2-nameid-mapper-unmeasured/session.anyMember"],
      ['identity.store();', "saml2-nameid-mapper-unmeasured/identity.store()"],
      ['identity.getAttributeValues("mail").size();', 'saml2-nameid-mapper-unmeasured/identity.getAttributeValues("mail").size'],
      ["nameIDScriptHelper.createNameIdentifier();", "saml2-nameid-mapper-unmeasured/nameIDScriptHelper.createNameIdentifier()"],
      ["nameIDScriptHelper.NAMEID_FORMAT_EMAIL;", "saml2-nameid-mapper-unmeasured/nameIDScriptHelper.NAMEID_FORMAT_EMAIL"],
      ['identity.getAttributeValues("uid");', "saml2-nameid-mapper-unmeasured/identity.getAttributeValues()"],
      ['nameIDScriptHelper.getNameIDValue("x");', "saml2-nameid-mapper-unmeasured/nameIDScriptHelper.getNameIDValue(string)"],
    ];
    for (const [script, id] of cases) {
      expectHarnessFault(id);
      const error = await runCase(runner, defineCase({
        name: id,
        context: "SAML2_NAMEID_MAPPER",
        script,
        given,
        expect: { nameId: { value: "unused" } },
      })).then(() => null, (caught: unknown) => caught);
      expect(error, id).toBeInstanceOf(CaseRunError);
    }
  }, 60_000);
});

// Consuming authoring-API checks, compiled by the package typecheck.
function authoringTypeChecks() {
  // @ts-expect-error NameID identity has neither universalId nor uid attributes
  defineCase({
    name: "unsupported identity fixture", script: '"ok";', context: "SAML2_NAMEID_MAPPER",
    given: { identity: { universalId: "id=x", attributes: { uid: ["x"] } } },
    expect: { nameId: { value: "ok" } },
  });
  defineCase({
    name: "unsupported universalId", script: '"ok";', context: "SAML2_NAMEID_MAPPER",
    // @ts-expect-error universalId was not measured in this context
    given: { identity: { universalId: "id=x" } },
    expect: { nameId: { value: "ok" } },
  });
  // @ts-expect-error the measured mail fixture contains exactly one string
  defineCase({
    name: "unsupported mail cardinality", script: '"ok";', context: "SAML2_NAMEID_MAPPER",
    given: { identity: { attributes: { mail: ["x", "y"] } } },
    expect: { nameId: { value: "ok" } },
  });
}

void authoringTypeChecks;
