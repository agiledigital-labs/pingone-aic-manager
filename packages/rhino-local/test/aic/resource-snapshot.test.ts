import { describe, expect, it } from "vitest";
import {
  confirmResourceSnapshot,
  nodeRequestProjection,
} from "../../src/aic/resource-snapshot.ts";

describe("AIC resource snapshots", () => {
  it("uses decoded source from the confirming read", () => {
    const submitted = script("expected");
    const confirmed = {
      ...script("confirmed"),
      createdBy: "server",
      creationDate: 1,
      lastModifiedBy: "server",
      lastModifiedDate: 2,
    };
    expect(() => confirmResourceSnapshot("script", submitted, confirmed)).toThrow(
      /confirming read did not match/
    );
  });

  it("never includes decoded script source in a mismatch", () => {
    const secretSource = "discriminating-source-value";
    expect(() =>
      confirmResourceSnapshot("script", script(secretSource), script("different"))
    ).toThrowError(expect.not.stringContaining(secretSource));
  });

  it("strips accepted but server-owned node metadata from requests", () => {
    expect(
      nodeRequestProjection({
        _id: "node",
        _type: { name: "wrong after read" },
        _outcomes: [{ id: "wrong after read" }],
        script: "script",
        outcomes: ["true"],
      })
    ).toEqual({ script: "script", outcomes: ["true"] });
  });

  it("accepts measured node and tree read-back additions", () => {
    const node = {
      script: "script",
      outcomes: ["true"],
      inputs: ["*"],
      outputs: ["*"],
    };
    expect(
      confirmResourceSnapshot("node", node, {
        ...node,
        _id: "node",
        _rev: "rev",
        _type: { server: true },
        _outcomes: [{ id: "true" }],
      })
    ).toEqual(node);

    const tree = {
      entryNodeId: "node",
      nodes: { node: { nodeType: "ScriptedDecisionNode", x: 1, y: 2 } },
    };
    expect(
      confirmResourceSnapshot("tree", tree, {
        ...tree,
        _id: "tree",
        _rev: "rev",
        innerTreeOnly: false,
        noSession: false,
        mustRun: false,
        transactionalOnly: false,
        nodes: {
          node: {
            nodeType: "ScriptedDecisionNode",
            x: 1,
            y: 2,
            version: "1.0",
          },
        },
      })
    ).toEqual(tree);
  });

  it("fails closed on an unknown functional response field", () => {
    expect(() =>
      confirmResourceSnapshot(
        "node",
        { script: "script", outcomes: ["true"] },
        { script: "script", outcomes: ["true"], silentlyAdded: true }
      )
    ).toThrow(/unexpected fields: silentlyAdded/);
  });
});

function script(source: string): Record<string, unknown> {
  return {
    _id: "script",
    name: "name",
    description: "marker",
    script: Buffer.from(source, "utf8").toString("base64"),
    default: false,
    language: "JAVASCRIPT",
    context: "AUTHENTICATION_TREE_DECISION_NODE",
    evaluatorVersion: "2.0",
  };
}
