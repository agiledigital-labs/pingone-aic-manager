import { describe, expect, it } from "vitest";
import { loadBehaviour, runScript } from "./load-behaviour.ts";

describe("systemEnv", () => {
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
  it("resolves a managed record by _id or userName", () => {
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
    expect(byName.getUniversalId()).toBe("uuid-1");
  });

  it("throws naming a missing identity", () => {
    expect(() => runScript('idRepository.getIdentity("nobody");')).toThrow(
      /no given\.managed record for "nobody"/
    );
  });
});
