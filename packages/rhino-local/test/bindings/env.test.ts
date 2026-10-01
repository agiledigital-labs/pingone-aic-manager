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
    expect(effects.openidm).toEqual([{ method: "patch", resource: "managed/alpha_user/uuid-1", body: [
      { operation: "replace", field: "frUnindexedString1", value: ["new"] },
      { operation: "replace", field: "frUnindexedMultivalued1", value: ["first", "second"] },
    ] }]);
    expect(effects.managedStore?.["managed/alpha_user"]?.[0]).toMatchObject({
      frUnindexedString1: "new", frUnindexedMultivalued1: ["first", "second"],
    });
  });
});
