import { describe, expect, it } from "vitest";
import { emitLeasedJourney } from "../../src/aic/emit-journey.ts";
import {
  createLeaseIdentity,
  uuidV5,
} from "../../src/aic/lease-identity.ts";

describe("AIC lease identity", () => {
  it("keeps addresses and the snapshot key stable across source edits and opens", () => {
    const first = createLeaseIdentity({
      id: "resolve-identity",
      source: 'action.goTo("one");',
      outcomes: ["one"],
      ownerToken: "owner-a",
    });
    const second = createLeaseIdentity({
      id: "resolve-identity",
      source: 'action.goTo("two");',
      outcomes: ["one"],
      ownerToken: "owner-b",
    });
    expect(second.treeName).toBe(first.treeName);
    expect(second.ids).toEqual(first.ids);
    expect(second.snapshotKey).toBe(first.snapshotKey);
    expect(second.authorDigest).not.toBe(first.authorDigest);
    expect(second.ownerToken).not.toBe(first.ownerToken);
  });

  it("adds only outcome-owned slots when the vocabulary grows", () => {
    const small = createLeaseIdentity({
      id: "outcomes",
      source: "source",
      outcomes: ["allow"],
      ownerToken: "owner",
    });
    const large = createLeaseIdentity({
      id: "outcomes",
      source: "source",
      outcomes: ["allow", "deny"],
      ownerToken: "owner",
    });
    expect(large.ids.subjectScript).toBe(small.ids.subjectScript);
    expect(large.ids.subjectNode).toBe(small.ids.subjectNode);
    expect(large.ids.resultScripts.allow).toBe(small.ids.resultScripts.allow);
    expect(large.ids.resultNodes.allow).toBe(small.ids.resultNodes.allow);
    expect(large.ids.resultScripts.deny).toBeDefined();
    expect(large.structuralDigest).not.toBe(small.structuralDigest);
  });

  it("emits a fixed graph without leaking seed values into request channels", () => {
    const identity = createLeaseIdentity({
      id: "fixed-graph",
      source: "author source",
      outcomes: ["done"],
      ownerToken: "owner",
    });
    const wrapper = emitLeasedJourney({
      identity,
      realm: "alpha",
      suiteName: "fixed graph",
    });
    expect(wrapper.treeName).toBe(identity.treeName);
    expect(wrapper.subjectOutcomes).toEqual(["true", "false", "done"]);
    expect(wrapper.invoke).toEqual({ headers: {}, parameters: {}, cookies: {} });
    expect(wrapper.scripts[0]?.source).not.toContain("author source");
    expect(wrapper.nodeBodies[identity.ids.subjectNode]).not.toHaveProperty("_type");
    expect(wrapper.nodeBodies[identity.ids.subjectNode]).not.toHaveProperty("_outcomes");
  });

  it("produces RFC 4122 version-5 UUIDs deterministically", () => {
    expect(uuidV5("subject-script")).toBe(uuidV5("subject-script"));
    expect(uuidV5("subject-script")).toMatch(
      /^[0-9a-f]{8}-[0-9a-f]{4}-5[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/
    );
  });
});
