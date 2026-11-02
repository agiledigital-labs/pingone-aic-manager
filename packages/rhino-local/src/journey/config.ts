/** Immutable REST config snapshot; journey instances and their forks share it. */
import type { JourneyConfigInput, NodeDocument, ScriptDocument, TreeDocument } from "./types.ts";
function freeze(value: unknown): void {
  if (value === null || typeof value !== "object") return;
  if (!Array.isArray(value)) Object.setPrototypeOf(value, null);
  for (const child of Object.values(value)) freeze(child);
  Object.freeze(value);
}
function record(value: unknown, label: string): asserts value is Record<string, unknown> {
  if (value === null || typeof value !== "object" || Array.isArray(value) || ![Object.prototype, null].includes(Object.getPrototypeOf(value) as object | null)) throw new TypeError(`Invalid journey configuration: ${label} must be a plain object`);
}
export class JourneyConfig {
  readonly realm: string;
  readonly trees: JourneyConfigInput["trees"];
  readonly nodes: JourneyConfigInput["nodes"];
  readonly scripts: JourneyConfigInput["scripts"];
  constructor(input: JourneyConfigInput) {
    record(input, "input");
    if (typeof input.realm !== "string" || !/^[a-zA-Z0-9_-]+$/.test(input.realm)) throw new TypeError("Invalid journey configuration: realm must be a simple realm name");
    for (const key of ["trees", "nodes", "scripts"] as const) record(input[key], key);
    for (const [name, tree] of Object.entries(input.trees)) {
      record(tree, `trees/${name}`); record(tree.nodes, `trees/${name}/nodes`);
      if (typeof tree.entryNodeId !== "string") throw new TypeError(`Invalid journey configuration: trees/${name}/entryNodeId`);
      for (const [id, node] of Object.entries(tree.nodes)) { record(node, `trees/${name}/${id}`); record(node.connections, `trees/${name}/${id}/connections`); }
    }
    for (const [type, nodes] of Object.entries(input.nodes)) { record(nodes, `nodes/${type}`); for (const [id, node] of Object.entries(nodes)) record(node, `nodes/${type}/${id}`); }
    for (const [id, script] of Object.entries(input.scripts)) record(script, `scripts/${id}`);
    const snapshot = structuredClone(input); freeze(snapshot);
    this.realm = snapshot.realm; this.trees = snapshot.trees; this.nodes = snapshot.nodes; this.scripts = snapshot.scripts;
    Object.freeze(this);
  }
  tree(name: string): TreeDocument | undefined { return this.trees[name]; }
  node(type: string, id: string): NodeDocument | undefined { return this.nodes[type]?.[id]; }
  script(id: string): ScriptDocument | undefined { return this.scripts[id]; }
}
