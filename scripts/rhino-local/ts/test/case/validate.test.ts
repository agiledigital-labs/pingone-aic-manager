import { describe, expect, it } from "vitest";
import { defineCase, validateCase } from "../../src/case/index.ts";

function validInput() {
  return {
    name: "grants access when the user has a verified email",
    script: "am/decision-node/check-email.js",
    given: {
      realm: "alpha",
      sharedState: { username: "alice" },
      transientState: {},
      secureState: {},
      requestHeaders: { "x-forwarded-for": ["10.0.0.1"] },
      requestParameters: { goto: ["https://example.com/app"] },
      requestCookies: {},
      resumedFromSuspend: false,
      esv: { "esv-feature-flag": "true" },
      secrets: { "my-signing-key": "placeholder" },
      managed: {
        alpha_user: [{ userName: "alice", mail: ["a@example.com"] }],
      },
      http: [
        {
          match: { url: /\/verify$/ },
          reply: { status: 200, body: {} },
        },
      ],
    },
    expect: {
      outcome: "true",
      sharedState: { added: { verified: true } },
      transientState: {},
      callbacks: [],
      openidm: [],
      http: [{ url: /\/verify$/, times: 1 }],
      logs: [],
    },
  };
}

describe("defineCase / validateCase", () => {
  it("accepts the design-note shape", () => {
    const kase = defineCase(validInput());
    expect(kase.name).toBe("grants access when the user has a verified email");
    expect(kase.script).toBe("am/decision-node/check-email.js");
    expect(kase.given.realm).toBe("alpha");
    expect(kase.expect.outcome).toBe("true");
    expect(kase.expect.sharedState).toEqual({ added: { verified: true } });
    expect(kase.given.http?.[0]?.match.url).toEqual(/\/verify$/);
  });

  it("defaults omitted given to an empty object", () => {
    const kase = defineCase({
      name: "outcome only",
      script: "am/decision-node/ok.js",
      expect: { outcome: "true" },
    });
    expect(kase.given).toEqual({});
  });

  it("rejects a missing name", () => {
    const input = validInput() as unknown as Record<string, unknown>;
    delete input.name;
    expect(() => validateCase(input)).toThrow(
      /rhino-local: case.name must be a non-empty string/
    );
  });

  it("rejects a missing script", () => {
    const input = validInput() as unknown as Record<string, unknown>;
    delete input.script;
    expect(() => validateCase(input)).toThrow(
      /rhino-local: case.script must be a non-empty string/
    );
  });

  it("rejects a missing expect", () => {
    const input = validInput() as unknown as Record<string, unknown>;
    delete input.expect;
    expect(() => validateCase(input)).toThrow(
      /rhino-local: case.expect is required/
    );
  });

  it("rejects a missing outcome rather than asserting nothing", () => {
    const input = validInput();
    const expectBlock = input.expect as unknown as Record<string, unknown>;
    delete expectBlock.outcome;
    expect(() => validateCase(input)).toThrow(
      /case.expect.outcome is required/
    );
  });

  it("rejects a typo in an expect channel (openidmm)", () => {
    const input = {
      name: "typo",
      script: "am/decision-node/x.js",
      expect: { outcome: "true", openidmm: [] },
    };
    expect(() => validateCase(input)).toThrow(
      /case.expect has unknown key "openidmm" \(did you mean "openidm"\?\)/
    );
  });

  it("rejects a typo in a given binding seed (requestHeader)", () => {
    const input = {
      name: "typo",
      script: "am/decision-node/x.js",
      given: { requestHeader: { "x-test": ["1"] } },
      expect: { outcome: "true" },
    };
    expect(() => validateCase(input)).toThrow(
      /case.given has unknown key "requestHeader" \(did you mean "requestHeaders"\?\)/
    );
  });

  it("rejects a typo in given.bindings against the generated surface", () => {
    const input = {
      name: "typo",
      script: "am/decision-node/x.js",
      given: { bindings: { openidmm: {} } },
      expect: { outcome: "true" },
    };
    expect(() => validateCase(input)).toThrow(
      /case.given.bindings has unknown key "openidmm" \(did you mean "openidm"\?\)/
    );
  });

  it("rejects seeding nodeState via bindings", () => {
    const input = {
      name: "nodeState",
      script: "am/decision-node/x.js",
      given: { bindings: { nodeState: {} } },
      expect: { outcome: "true" },
    };
    expect(() => validateCase(input)).toThrow(
      /given.bindings.nodeState is not a seed; use given.sharedState/
    );
  });

  it("rejects a binding seed that collides with a dedicated given field", () => {
    const input = {
      name: "collide",
      script: "am/decision-node/x.js",
      given: { bindings: { realm: "alpha" } },
      expect: { outcome: "true" },
    };
    expect(() => validateCase(input)).toThrow(
      /case.given.bindings.realm collides with given.realm/
    );
  });

  it("rejects an unknown allowUndeclared channel", () => {
    const input = {
      name: "flags",
      script: "am/decision-node/x.js",
      expect: {
        outcome: "true",
        allowUndeclared: { openidm: true },
      },
    };
    expect(() => validateCase(input)).toThrow(
      /case.expect.allowUndeclared has unknown key "openidm" \(did you mean "openidmWrites"\?\)/
    );
  });

  it("rejects an invalid engine", () => {
    const input = {
      name: "engine",
      script: "am/decision-node/x.js",
      given: { engine: "rhino" },
      expect: { outcome: "true" },
    };
    expect(() => validateCase(input)).toThrow(
      /case.given.engine must be "next-gen" or "legacy"/
    );
  });

  it("rejects actionName on a non-action openidm expectation", () => {
    const input = {
      name: "actionName",
      script: "am/decision-node/x.js",
      expect: {
        outcome: "true",
        openidm: [
          {
            method: "create",
            resource: "managed/alpha_user",
            actionName: "create",
          },
        ],
      },
    };
    expect(() => validateCase(input)).toThrow(
      /actionName is only valid when method is "action"/
    );
  });

  it("rejects a negative times count", () => {
    const input = {
      name: "times",
      script: "am/decision-node/x.js",
      expect: {
        outcome: "true",
        http: [{ url: "https://example.com", times: -1 }],
      },
    };
    expect(() => validateCase(input)).toThrow(
      /case.expect.http\[0\].times must be a non-negative integer/
    );
  });

  it("rejects a header value that is not a string array", () => {
    const input = {
      name: "headers",
      script: "am/decision-node/x.js",
      given: { requestHeaders: { accept: "application/json" } },
      expect: { outcome: "true" },
    };
    expect(() => validateCase(input)).toThrow(
      /case.given.requestHeaders.accept must be an array of strings/
    );
  });

  it("rejects a dynamically built case whose expect channel is misspelt", () => {
    const built: Record<string, unknown> = {
      name: "dynamic",
      script: "am/decision-node/x.js",
      expect: { outcome: "true" },
    };
    (built.expect as Record<string, unknown>).openidmm = [];
    expect(() => validateCase(built)).toThrow(/unknown key "openidmm"/);
  });

  it("accepts a known extra binding seed", () => {
    const kase = defineCase({
      name: "binding seed",
      script: "am/decision-node/x.js",
      given: { bindings: { logger: {} } },
      expect: { outcome: "true" },
    });
    expect(kase.given.bindings).toEqual({ logger: {} });
  });
});
