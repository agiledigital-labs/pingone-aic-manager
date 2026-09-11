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
