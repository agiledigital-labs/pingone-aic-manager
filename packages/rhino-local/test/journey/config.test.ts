import { expect, it } from "vitest";
import { JourneyConfig } from "../../src/journey/index.ts";
it("journey config uses immutable REST shapes and own names", () => {
  const input = { realm: "alpha", trees: { invented: { entryNodeId: "node", nodes: { node: { nodeType: "ScriptedDecisionNode", connections: {} } } } }, nodes: { ScriptedDecisionNode: { node: { script: "script", outcomes: ["true"] } } }, scripts: { script: { evaluatorVersion: "2.0", script: Buffer.from("action.goTo('true');").toString("base64") } } };
  const config = new JourneyConfig(input);
  input.nodes.ScriptedDecisionNode.node.outcomes.push("false");
  expect(config.node("ScriptedDecisionNode", "node")?.outcomes).toEqual(["true"]);
  expect(config.tree("constructor")).toBeUndefined();
  expect(Object.isFrozen(config.tree("invented")?.nodes)).toBe(true);
});
it.each([null, [], false, 0])("journey config rejects malformed maps %j", (trees) => {
  expect(() => new JourneyConfig({ realm: "alpha", trees, nodes: {}, scripts: {} } as unknown as ConstructorParameters<typeof JourneyConfig>[0])).toThrow("trees must be a plain object");
});
