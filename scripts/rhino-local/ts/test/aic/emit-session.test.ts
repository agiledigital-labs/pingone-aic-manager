import { describe, expect, it } from "vitest";
import { emitSessionJourney } from "../../src/aic/emit-session.ts";
import { SUCCESS_NODE_ID } from "../../src/aic/constants.ts";

const OPTIONS = { runId: "run123", realm: "alpha" };

describe("emitSessionJourney", () => {
  it("authenticates as the principal and sets only the custom properties", () => {
    const journey = emitSessionJourney(
      {
        UserId: "alice",
        Principals: "alice",
        "sun.am.UniversalIdentifier": "id=alice,ou=user,o=alpha,ou=services,ou=am-config",
        tier: "gold",
        step: "2",
      },
      OPTIONS
    );
    const source = journey.scripts[0]?.source ?? "";
    expect(source).toContain('nodeState.putShared("username", "alice");');
    expect(source).toContain('.putSessionProperty("tier", "gold")');
    expect(source).toContain('.putSessionProperty("step", "2")');
    // The discriminating assertion: sending an AM-owned property does not
    // override it, it fails the whole login with a bare 401.
    expect(source).not.toContain("UserId");
    expect(source).not.toContain("UniversalIdentifier");
  });

  it("still mints a session when no custom properties are declared", () => {
    const source = emitSessionJourney({ UserId: "alice" }, OPTIONS).scripts[0]?.source ?? "";
    expect(source).toContain('action.goTo("true")');
    expect(source).not.toContain("putSessionProperty");
  });

  it("falls back to the fixed principal when the map names none", () => {
    const source = emitSessionJourney({ tier: "gold" }, OPTIONS).scripts[0]?.source ?? "";
    expect(source).toContain('nodeState.putShared("username", "rl-session");');
  });

  it("builds a one-node tree that runs to completion", () => {
    const journey = emitSessionJourney({ tier: "gold" }, OPTIONS);
    expect(journey.treeName).toBe("rl-aic-run123-session");
    expect(journey.nodes).toHaveLength(1);
    const node = journey.nodes[0];
    // Reaching Success is the whole point: no session is issued otherwise.
    expect(node?.connections).toEqual({ true: SUCCESS_NODE_ID });
    expect(journey.treeBody.entryNodeId).toBe(node?.id);
    expect(journey.treeBody.noSession).toBe(false);
  });

  it("escapes values rather than pasting them into the source", () => {
    const source = emitSessionJourney({ note: 'a"b\\c' }, OPTIONS).scripts[0]?.source ?? "";
    expect(source).toContain('.putSessionProperty("note", "a\\"b\\\\c")');
  });
});
