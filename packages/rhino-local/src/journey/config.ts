/** Immutable REST config snapshot; journey instances and their forks share it. */
import { validateExtraNode } from "./extra-nodes.ts";
import type { JourneyConfigInput, NodeDocument, ScriptDocument, TreeDocument } from "./types.ts";
function freeze(value: unknown): void {
  if (value === null || typeof value !== "object") return;
  if (!Array.isArray(value)) Object.setPrototypeOf(value, null);
  for (const child of Object.values(value)) freeze(child);
  Object.freeze(value);
}
function record(value: unknown, label: string): asserts value is Record<string, unknown> {
  if (
    value === null ||
    typeof value !== "object" ||
    Array.isArray(value) ||
    ![Object.prototype, null].includes(Object.getPrototypeOf(value) as object | null)
  )
    throw new TypeError(`Invalid journey configuration: ${label} must be a plain object`);
}
export class JourneyConfig {
  readonly realm: string;
  readonly trees: JourneyConfigInput["trees"];
  readonly nodes: JourneyConfigInput["nodes"];
  readonly scripts: JourneyConfigInput["scripts"];
  readonly customNodeTypes: NonNullable<JourneyConfigInput["customNodeTypes"]>;
  constructor(input: JourneyConfigInput) {
    record(input, "input");
    if (typeof input.realm !== "string" || !/^[a-zA-Z0-9_-]+$/.test(input.realm))
      throw new TypeError("Invalid journey configuration: realm must be a simple realm name");
    for (const key of ["trees", "nodes", "scripts"] as const) record(input[key], key);
    if (input.customNodeTypes !== undefined) {
      record(input.customNodeTypes, "customNodeTypes");
      // The script is the type: a type without one has nothing to run, so it is refused here rather
      // than failing inside the engine. What the tenant does with such a type is unmeasured.
      for (const [name, def] of Object.entries(input.customNodeTypes)) {
        record(def, `customNodeTypes/${name}`);
        if (typeof def.script !== "string")
          throw new TypeError(`Invalid journey configuration: customNodeTypes/${name}/script must be a string`);
      }
    }
    for (const [name, tree] of Object.entries(input.trees)) {
      record(tree, `trees/${name}`);
      record(tree.nodes, `trees/${name}/nodes`);
      if (typeof tree.entryNodeId !== "string")
        throw new TypeError(`Invalid journey configuration: trees/${name}/entryNodeId`);
      for (const [id, node] of Object.entries(tree.nodes)) {
        record(node, `trees/${name}/${id}`);
        record(node.connections, `trees/${name}/${id}/connections`);
      }
    }
    for (const [type, nodes] of Object.entries(input.nodes)) {
      record(nodes, `nodes/${type}`);
      for (const [id, node] of Object.entries(nodes)) {
        record(node, `nodes/${type}/${id}`);
        validateExtraNode(type, node);
      }
    }
    for (const [id, script] of Object.entries(input.scripts)) record(script, `scripts/${id}`);
    for (const page of Object.values(input.nodes.PageNode ?? {})) {
      if (!Array.isArray(page.nodes)) continue;
      // Measured (docs/api/09-journeys.md, PageNode children): a page cannot hold a page.
      for (const child of page.nodes)
        if (child !== null && typeof child === "object" && (child as Record<string, unknown>).nodeType === "PageNode")
          throw new RangeError("400 Invalid config: Illegal child node type: PageNode");
      for (const child of page.nodes.slice(0, -1)) {
        if (child === null || typeof child !== "object" || Array.isArray(child)) continue;
        const ref = child as Record<string, unknown>;
        const config =
          typeof ref.nodeType === "string" &&
          typeof ref._id === "string" &&
          Object.hasOwn(input.nodes, ref.nodeType) &&
          Object.hasOwn(input.nodes[ref.nodeType] ?? {}, ref._id)
            ? input.nodes[ref.nodeType]?.[ref._id]
            : undefined;
        const outcomes =
          ref.nodeType === "InnerTreeEvaluatorNode" || ref.nodeType === "MessageNode"
            ? ["true", "false"]
            : config?.outcomes;
        if (Array.isArray(outcomes) && outcomes.length > 1)
          throw new RangeError(
            "400 Invalid config: Only the last node in a page can have more than one outcome",
          );
        if (
          ["IdentifyExistingUserNode", "CreateObjectNode", "PatchObjectNode"].includes(
            String(ref.nodeType),
          )
        )
          throw new RangeError(
            "400 Invalid config: Only the last node in a page can have more than one outcome",
          );
      }
    }
    const snapshot = structuredClone(input);
    freeze(snapshot);
    this.realm = snapshot.realm;
    this.trees = snapshot.trees;
    this.nodes = snapshot.nodes;
    this.scripts = snapshot.scripts;
    this.customNodeTypes = snapshot.customNodeTypes ?? {};
    Object.freeze(this);
  }
  tree(name: string): TreeDocument | undefined {
    return this.trees[name];
  }
  node(type: string, id: string): NodeDocument | undefined {
    return this.nodes[type]?.[id];
  }
  script(id: string): ScriptDocument | undefined {
    return this.scripts[id];
  }
}
