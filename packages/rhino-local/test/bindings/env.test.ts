import { describe, expect, it } from "vitest";
import { loadBehaviour, runScript } from "./load-behaviour.ts";

describe("systemEnv", () => {
  it("lets a local replacement wrap and call through to the original binding", () => {
    const effects = runScript(
      [
        'var value = systemEnv.getProperty("esv.flag");',
        'action.goTo(value === "on" && systemEnv.callCount() === 1 ? "match" : "mismatch");',
      ].join("\n"),
      {
        esv: { "esv.flag": "on" },
        bindingOverrides: { systemEnv: [
          "(function (original) {",
          "  var count = 0;",
          "  return {",
          "    getProperty: function () { count += 1; return original.getProperty.apply(original, arguments); },",
          "    callCount: function () { return count; }",
          "  };",
          "})(systemEnv)",
        ].join("\n") },
      }
    );
    expect(effects.outcome).toBe("match");
  });
  it("looks up given.esv", () => {
    const sandbox = loadBehaviour({ esv: { "esv.foo": "bar" } });
    const systemEnv = sandbox.systemEnv as {
      getProperty: (key: string) => string;
    };
    expect(systemEnv.getProperty("esv.foo")).toBe("bar");
  });

  it("throws naming a missing esv key", () => {
    expect(() => runScript('systemEnv.getProperty("esv.missing");')).toThrow(
      /no given\.esv entry for "esv\.missing"/
    );
  });

  // Regression: one declared ESV used to make every other key read as null.
  it("throws for an undeclared key even when another ESV is seeded", () => {
    const sandbox = loadBehaviour({ esv: { "esv.known": "value" } });
    const systemEnv = sandbox.systemEnv as { getProperty: (key: string) => string };
    expect(() => systemEnv.getProperty("esv.missing")).toThrow(
      /no given\.esv entry for "esv\.missing"/
    );
  });

  it("returns null or the supplied default for an explicitly absent ESV", () => {
    const sandbox = loadBehaviour({ esv: { "esv.absent": null } });
    const systemEnv = sandbox.systemEnv as {
      getProperty: (key: string, fallback?: string) => string | null;
    };
    expect(systemEnv.getProperty("esv.absent")).toBeNull();
    expect(systemEnv.getProperty("esv.absent", "fallback")).toBe("fallback");
  });
});

describe("secrets", () => {
  it("returns the seeded secret via getAsUtf8, not a neighbour", () => {
    const sandbox = loadBehaviour({
      secrets: {
        "esv.api-key": "alpha",
        "esv.other": "bravo",
      },
    });
    type Secret = { getAsUtf8: () => string };
    const secrets = sandbox.secrets as {
      getGenericSecret: (id: string) => Secret;
      getDecryptionKey: (id: string) => Secret;
      getEncryptionKey: (id: string) => Secret;
      getSigningKey: (id: string) => Secret;
      getVerificationKey: (id: string) => Secret;
    };
    expect(secrets.getGenericSecret("esv.api-key").getAsUtf8()).toBe("alpha");
    expect(secrets.getDecryptionKey("esv.api-key").getAsUtf8()).toBe("alpha");
    expect(secrets.getEncryptionKey("esv.api-key").getAsUtf8()).toBe("alpha");
    expect(secrets.getSigningKey("esv.other").getAsUtf8()).toBe("bravo");
    expect(secrets.getVerificationKey("esv.other").getAsUtf8()).toBe("bravo");
  });

  it("throws naming a missing given.secrets key", () => {
    expect(() =>
      runScript('secrets.getGenericSecret("esv.missing").getAsUtf8();')
    ).toThrow(/no given\.secrets entry for "esv\.missing"/);
  });
});

describe("idRepository", () => {
  it("resolves a managed record by _id", () => {
    const sandbox = loadBehaviour({
      managed: {
        "managed/alpha_user": [
          { _id: "uuid-1", userName: "alice", mail: "alice@example.com" },
        ],
      },
    });
    const idRepository = sandbox.idRepository as {
      getIdentity: (id: string) => {
        getName: () => string;
        getUniversalId: () => string;
        exists: () => boolean;
        getAttributeValues: (name: string) => { get: (i: number) => string };
      };
    };
    const byId = idRepository.getIdentity("uuid-1");
    expect(byId.getName()).toBe("alice");
    expect(byId.getUniversalId()).toBe("uuid-1");
    expect(byId.exists()).toBe(true);
    expect(byId.getAttributeValues("mail").get(0)).toBe("alice@example.com");
    const byName = idRepository.getIdentity("alice");
    expect(byName.exists()).toBe(false);
  });

  it("returns a non-null stub whose attribute operations fail on null amIdentity", () => {
    const sandbox = loadBehaviour();
    const repository = sandbox.idRepository as {
      getIdentity: (id: string) => {
        getAttributeValues: (name: string) => unknown;
        setAttribute: (name: string, values: string[]) => unknown;
        store: () => unknown;
        getAttribute?: unknown;
      };
    };
    const identity = repository.getIdentity("missing-id");
    expect(identity).not.toBeNull();
    expect(typeof identity.getAttribute).toBe("undefined");
    expect(Object.keys(identity).sort()).toEqual([
      "addAttribute", "exists", "getAttributeValues", "getName",
      "getUniversalId", "setAttribute", "store", "toString",
    ].sort());
    expect(() => identity.getAttributeValues("mail")).toThrow(/getAttributeValues\(String\).*this\.amIdentity.*null/);
    expect(() => identity.setAttribute("mail", ["a@example.com"])).toThrow(/setAttribute\(String, String\[\]\).*this\.amIdentity.*null/);
    expect(() => identity.store()).toThrow(/storeAndThrow\(\).*this\.amIdentity.*null/);
  });

  it("keeps staged writes invisible to reads until store and records the persisted change", () => {
    const effects = runScript(
      [
        'var identity = idRepository.getIdentity("uuid-1");',
        'identity.setAttribute("fr-attr-str1", ["new"]);',
        'if (identity.getAttributeValues("fr-attr-str1").toArray()[0] !== "old") throw new Error("persisted value missing");',
        'identity.addAttribute("fr-attr-multi1", "second");',
        'identity.store();',
        'if (identity.getAttributeValues("fr-attr-str1").toArray()[0] !== "new") throw new Error("stored value missing");',
        'if (!identity.getAttributeValues("fr-attr-multi1").contains("second")) throw new Error("added value missing");',
      ].join("\n"),
      { managed: { "managed/alpha_user": [{ _id: "uuid-1", frUnindexedString1: "old", frUnindexedMultivalued1: ["first"] }] } }
    );
    // Regression: store() used to be recorded as an invented openidm patch.
    // AM persists through its identity repository, so it is an identity
    // write named by AM attribute, and the openidm channel stays empty.
    expect(effects.openidm).toEqual([]);
    expect(effects.identityWrites).toEqual([
      { identity: "uuid-1", attribute: "fr-attr-str1", values: ["new"] },
      { identity: "uuid-1", attribute: "fr-attr-multi1", values: ["first", "second"] },
    ]);
    expect(effects.managedStore?.["managed/alpha_user"]?.[0]).toMatchObject({
      frUnindexedString1: "new", frUnindexedMultivalued1: ["first", "second"],
    });
  });

  it("enumerates only indices on a getAttributeValues toArray()", () => {
    const effects = runScript(
      [
        'var keys = [];',
        'var arr = idRepository.getIdentity("uuid-1").getAttributeValues("fr-attr-multi1").toArray();',
        'for (var k in arr) { keys.push(k); }',
        'nodeState.putShared("keys", keys.join(","));',
        'nodeState.putShared("length", arr.length);',
      ].join("\n"),
      { managed: { "managed/alpha_user": [{ _id: "uuid-1", frUnindexedMultivalued1: ["a", "b"] }] } }
    );
    expect(effects.sharedState.final).toMatchObject({ keys: "0,1", length: 2 });
  });

  it("keeps store() out of the openidm channel and its failure stubs", () => {
    const effects = runScript(
      [
        'var identity = idRepository.getIdentity("uuid-1");',
        'identity.setAttribute("mail", ["new@example.com"]);',
        'identity.store();',
        'try { openidm.patch("managed/alpha_user/uuid-1", null, [{ operation: "replace", field: "sn", value: "x" }]); }',
        'catch (e) { nodeState.putShared("failed", true); }',
      ].join("\n"),
      {
        managed: { "managed/alpha_user": [{ _id: "uuid-1", mail: "old@example.com" }] },
        openidmFailures: [{ match: { method: "patch", resource: "managed/alpha_user/uuid-1", ordinal: 1 }, reply: { code: 500 } }],
      }
    );
    expect(effects.sharedState.final.failed).toBe(true);
    expect(effects.openidm).toHaveLength(1);
    expect(effects.identityWrites).toEqual([
      { identity: "uuid-1", attribute: "mail", values: ["new@example.com"] },
    ]);
  });

  it.each([
    ["setAttribute", 'identity.setAttribute("userName", ["bob"]);', "uid"],
    ["addAttribute", 'identity.addAttribute("frUnindexedString2", "x");', "fr-attr-str2"],
  ])("refuses an IDM field name passed to %s rather than writing a sentinel", (_method, call, am) => {
    // Regression: the write used to land on a "<not-an-AM-attribute>" field.
    expect(() => runScript(
      ['var identity = idRepository.getIdentity("uuid-1");', call, "identity.store();"].join("\n"),
      { managed: { "managed/alpha_user": [{ _id: "uuid-1", userName: "alice" }] } }
    )).toThrow(new RegExp(`IDM field name; AM names this attribute "${am}"`));
  });

  // Measured 2026-10-01 (live-identity-store-visibility): one value on a
  // multivalued attribute is stored as a one-element array, not a scalar.
  it.each([
    ["fr-attr-multi1", "frUnindexedMultivalued1", ["only"], ["only"]],
    ["fr-attr-multi1", "frUnindexedMultivalued1", ["a", "b"], ["a", "b"]],
    ["fr-attr-str1", "frUnindexedString1", ["new"], "new"],
    ["mail", "mail", ["new@example.com"], "new@example.com"],
  ])("store() keeps %s's IDM cardinality for %j", (attribute, field, values, stored) => {
    const effects = runScript(
      [
        'var identity = idRepository.getIdentity("uuid-1");',
        `identity.setAttribute(${JSON.stringify(attribute)}, ${JSON.stringify(values)});`,
        "identity.store();",
      ].join("\n"),
      { managed: { "managed/alpha_user": [{ _id: "uuid-1", frUnindexedMultivalued1: ["x", "y"] }] } }
    );
    expect(effects.managedStore?.["managed/alpha_user"]?.[0]?.[field]).toEqual(stored);
  });

  // Measured 2026-10-01 on alpha (live-identity-store-families): one row per
  // family representative and standard attribute, as the IDM property reads.
  it.each([
    ["fr-attr-istr1", ["v"], "frIndexedString1", "v"],
    ["fr-attr-istr20", ["v"], "frIndexedString20", "v"],
    ["fr-attr-istr1", [], "frIndexedString1", undefined],
    ["fr-attr-imulti1", ["only"], "frIndexedMultivalued1", ["only"]],
    ["fr-attr-imulti1", [], "frIndexedMultivalued1", []],
    ["fr-attr-multi1", [], "frUnindexedMultivalued1", []],
    ["fr-attr-iint1", ["42"], "frIndexedInteger1", 42],
    ["fr-attr-int1", ["7"], "frUnindexedInteger1", 7],
    ["fr-attr-idate1", ["20261001120000Z"], "frIndexedDate1", "2026-10-01T12:00:00Z"],
    ["fr-attr-date1", ["20261005083000Z"], "frUnindexedDate1", "2026-10-05T08:30:00Z"],
    ["givenName", ["Given"], "givenName", "Given"],
    ["givenName", ["x", "y"], "givenName", ["x", "y"]],
    ["givenName", [], "givenName", undefined],
    ["sn", ["p", "q"], "sn", ["p", "q"]],
    ["telephoneNumber", ["1"], "telephoneNumber", "1"],
    ["mail", ["p@example.com", "q@example.com"], "mail", ["p@example.com", "q@example.com"]],
  ])("store() lays %s = %j out as IDM %s", (attribute, values, field, stored) => {
    const effects = runScript(
      [
        'var identity = idRepository.getIdentity("uuid-1");',
        `identity.setAttribute(${JSON.stringify(attribute)}, ${JSON.stringify(values)});`,
        "identity.store();",
      ].join("\n"),
      { managed: { "managed/alpha_user": [{ _id: "uuid-1", [field]: "seeded" }] } }
    );
    expect(effects.managedStore?.["managed/alpha_user"]?.[0]?.[field]).toEqual(stored);
    expect(effects.identityWrites).toEqual([{ identity: "uuid-1", attribute, values }]);
  });

  it("reads a date back in the GeneralizedTime the script wrote, and an integer as a string", () => {
    const effects = runScript(
      [
        'var identity = idRepository.getIdentity("uuid-1");',
        'nodeState.putShared("date", identity.getAttributeValues("fr-attr-idate1").toArray()[0]);',
        'nodeState.putShared("int", identity.getAttributeValues("fr-attr-int1").toArray()[0]);',
        'nodeState.putShared("mapped", identity.getAttributeValues("fr-attr-int1").toArray().map(function (v) { return v + "!"; }).join());',
      ].join("\n"),
      { managed: { "managed/alpha_user": [{ _id: "uuid-1", frIndexedDate1: "2026-10-01T12:00:00Z", frUnindexedInteger1: 42 }] } }
    );
    expect(effects.sharedState.final).toMatchObject({ date: "20261001120000Z", int: "42", mapped: "42!" });
  });

  const updateError = (code: number) =>
    "JavaException: org.forgerock.openam.scripting.api.identity.ScriptedIdentityScriptWrapper$IdentityUpdateException: " +
    "Exception persisting attribute: Plug-in org.forgerock.openam.idrepo.ldap.DJLDAPv3Repo encountered a ldap exception.  " +
    `ldap errorcode=${code}`;

  // The script sees AM's exception, so its own error handling can be tested;
  // nothing in the store is applied.
  it.each([
    ["two values on a single-valued family", 'identity.setAttribute("fr-attr-str1", ["a", "b"]);', 65],
    ["no value on sn, which DS requires", 'identity.setAttribute("sn", []);', 65],
    ["a non-integer", 'identity.setAttribute("fr-attr-iint1", ["4.5"]);', 21],
    ["an ISO timestamp for a date", 'identity.setAttribute("fr-attr-date1", ["2026-10-02T12:00:00Z"]);', 21],
    ["a bare date", 'identity.setAttribute("fr-attr-idate1", ["2026-10-03"]);', 21],
    ["one bad attribute beside a good one", 'identity.setAttribute("fr-attr-str3", ["good"]); identity.setAttribute("fr-attr-int2", ["bad"]);', 21],
  ])("throws AM's IdentityUpdateException for %s", (_name, call, code) => {
    const effects = runScript(
      [
        'var identity = idRepository.getIdentity("uuid-1");',
        call,
        'try { identity.store(); nodeState.putShared("result", "ok"); } catch (e) { nodeState.putShared("result", String(e)); }',
      ].join("\n"),
      { managed: { "managed/alpha_user": [{ _id: "uuid-1", sn: "Probe", frUnindexedString1: "old" }] } }
    );
    expect(effects.sharedState.final.result).toBe(updateError(code));
    expect(effects.identityWrites).toEqual([]);
    expect(effects.managedStore?.["managed/alpha_user"]?.[0]).toEqual({ _id: "uuid-1", sn: "Probe", frUnindexedString1: "old" });
  });

  it.each([
    ["an attribute with no measurement", 'identity.setAttribute("displayName", ["Bob"]);', /how AM stores "displayName" in IDM is unmeasured.*identityAttributes: \{ "displayName": \{ field: "<IDM property>", cardinality: "single" \| "multi" \} \}/],
    ["cn, which AM keeps outside IDM", 'identity.setAttribute("cn", ["Bob"]);', /AM stores "cn" outside the IDM managed record/],
    ["a negative integer", 'identity.setAttribute("fr-attr-int1", ["-5"]);', /"-5" \(a signed, zero-padded or out-of-range integer\)/],
    ["a GeneralizedTime with no seconds", 'identity.setAttribute("fr-attr-date1", ["202610011200Z"]);', /GeneralizedTime other than YYYYMMDDHHMMSSZ/],
    ["two DS rules broken at once", 'identity.setAttribute("fr-attr-int1", ["x", "y"]);', /two DS rules at once \(ldap errorcodes 65 and 21\)/],
  ])("refuses %s", (_name, call, message) => {
    expect(() => runScript(
      ['var identity = idRepository.getIdentity("uuid-1");', call, "identity.store();"].join("\n"),
      { managed: { "managed/alpha_user": [{ _id: "uuid-1" }] } }
    )).toThrow(message);
  });

  describe("declared identityAttributes", () => {
    const store = (call: string, identityAttributes: Record<string, { field: string; cardinality: "single" | "multi" }>) => runScript(
      ['var identity = idRepository.getIdentity("uuid-1");', call, "identity.store();",
        'nodeState.putShared("read", JSON.stringify(identity.getAttributeValues("custom-attr").toArray().map(String)));'].join("\n"),
      { managed: { "managed/alpha_user": [{ _id: "uuid-1" }] }, identityAttributes }
    );

    it("adds an attribute the harness has no measurement for", () => {
      const single = store('identity.setAttribute("custom-attr", ["x"]);', { "custom-attr": { field: "custTenantField", cardinality: "single" } });
      expect(single.managedStore?.["managed/alpha_user"]?.[0]?.custTenantField).toBe("x");
      expect(single.sharedState.final.read).toBe('["x"]');
      const multi = store('identity.setAttribute("custom-attr", ["x"]);', { "custom-attr": { field: "custTenantField", cardinality: "multi" } });
      expect(multi.managedStore?.["managed/alpha_user"]?.[0]?.custTenantField).toEqual(["x"]);
    });

    it("overrides a measured default", () => {
      const effects = runScript(
        'var identity = idRepository.getIdentity("uuid-1"); identity.setAttribute("fr-attr-str1", ["x"]); identity.store();',
        { managed: { "managed/alpha_user": [{ _id: "uuid-1" }] }, identityAttributes: { "fr-attr-str1": { field: "elsewhere", cardinality: "multi" } } }
      );
      expect(effects.managedStore?.["managed/alpha_user"]?.[0]).toEqual({ _id: "uuid-1", elsewhere: ["x"] });
    });

    it("still refuses several values on a declared single-valued attribute", () => {
      expect(() => store('identity.setAttribute("custom-attr", ["x", "y"]);', { "custom-attr": { field: "custTenantField", cardinality: "single" } }))
        .toThrow(/what AM stores for 2 values of "custom-attr" is unmeasured/);
    });

    it("leaves an undeclared attribute refused", () => {
      expect(() => store('identity.setAttribute("other-attr", ["x"]);', { "custom-attr": { field: "custTenantField", cardinality: "single" } }))
        .toThrow(/how AM stores "other-attr" in IDM is unmeasured/);
    });
  });
});
