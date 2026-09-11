import { describe, expect, it } from "vitest";
import { emitSetupScript } from "../../src/aic/emit-setup.ts";
import { lintAmScript } from "./helpers.ts";

describe("emitSetupScript", () => {
  it("puts shared keys via putShared and transient keys via putTransient", () => {
    const source = emitSetupScript({
      sharedState: { username: "alice" },
      transientState: { password: "secret" },
    });
    expect(source).toContain("nodeState.putShared(key, value)");
    expect(source).toContain("nodeState.putTransient(key, value)");
    expect(source).toContain("action.goTo(\"true\")");
    expect(source).toContain("username");
    expect(source).toContain("alice");
    expect(source).toContain("password");
  });

  it("does not assign requestHeaders, requestParameters, or requestCookies", () => {
    const source = emitSetupScript({
      requestHeaders: { "x-forwarded-for": ["10.0.0.1"] },
      requestParameters: { goto: ["https://example.com/app"] },
      requestCookies: { sid: "abc" },
      sharedState: { username: "alice" },
    });
    expect(source).not.toMatch(/requestHeaders\s*=/);
    expect(source).not.toMatch(/requestParameters\s*=/);
    expect(source).not.toMatch(/requestCookies\s*=/);
    expect(source).not.toContain("x-forwarded-for");
    expect(source).not.toContain("10.0.0.1");
  });

  it("still goTo's when given state is empty, so the graph is uniform", () => {
    const source = emitSetupScript({});
    expect(source).toContain("action.goTo(\"true\")");
    expect(source).toContain("JSON.parse(\"{}\")");
  });

  it("passes AM Rhino lint", async () => {
    const source = emitSetupScript({
      sharedState: { username: "alice", nested: { ok: true } },
      transientState: { one: 1 },
    });
    expect(await lintAmScript(source, "generated/aic-setup.cjs")).toEqual([]);
  });
});
