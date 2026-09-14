/** Request projections and confirmed-read snapshots for leased AM resources. */
import { deepEqual } from "../case/equal.ts";
import { isPlainObject } from "../case/util.ts";
import { AicLaneError } from "./tenant.ts";

export type AicResourceKind = "script" | "node" | "tree";

const SCRIPT_SERVER_FIELDS = new Set([
  "createdBy",
  "creationDate",
  "lastModifiedBy",
  "lastModifiedDate",
  "_rev",
]);
const NODE_SERVER_FIELDS = new Set(["_id", "_rev", "_type", "_outcomes"]);
const TREE_SERVER_FIELDS = new Set([
  "_id",
  "_rev",
  "innerTreeOnly",
  "noSession",
  "mustRun",
  "transactionalOnly",
]);

/** Strip server-owned node metadata even though AM accepts some of it. */
export function nodeRequestProjection(
  value: Readonly<Record<string, unknown>>
): Record<string, unknown> {
  const { _id: _ignoredId, _type: _ignoredType, _outcomes: _ignoredOutcomes, _rev: _ignoredRev, ...body } = value;
  return body;
}

export function resourceRequestProjection(
  kind: AicResourceKind,
  value: Readonly<Record<string, unknown>>
): Record<string, unknown> {
  return kind === "node" ? nodeRequestProjection(value) : { ...value };
}

/**
 * Return the snapshot from the confirming GET, never from submitted bytes.
 *
 * Script and tree `description` round-trips byte-exact (measured 2026-09-14,
 * `docs/api/04-scripts.md`), so the marker stays in the comparison rather than
 * being normalized away — a rewritten marker is a real failure, not noise.
 */
export function confirmResourceSnapshot(
  kind: AicResourceKind,
  submitted: Readonly<Record<string, unknown>>,
  confirmed: unknown
): Record<string, unknown> {
  const expected = normalize(kind, resourceRequestProjection(kind, submitted), submitted);
  const actual = normalize(kind, confirmed, submitted);
  if (!deepEqual(expected, actual)) {
    throw new AicLaneError(
      `${kind} confirming read did not match the submitted functional content`
    );
  }
  return actual;
}

function normalize(
  kind: AicResourceKind,
  value: unknown,
  submitted: Readonly<Record<string, unknown>>
): Record<string, unknown> {
  if (!isPlainObject(value)) {
    throw new AicLaneError(`${kind} confirming read was not a JSON object`);
  }
  if (kind === "script") {
    return normalizeScript(value);
  }
  if (kind === "node") {
    return normalizeNode(value);
  }
  return normalizeTree(value, submitted);
}

function normalizeScript(value: Record<string, unknown>): Record<string, unknown> {
  const allowed = new Set([
    "_id",
    "name",
    "description",
    "script",
    "default",
    "language",
    "context",
    "evaluatorVersion",
  ]);
  rejectUnknown("script", value, allowed, SCRIPT_SERVER_FIELDS);
  if (typeof value.script !== "string") {
    throw new AicLaneError("script confirming read has no base64 source");
  }
  const normalized: Record<string, unknown> = {};
  for (const key of allowed) {
    if (key === "script") {
      normalized.source = Buffer.from(value.script, "base64").toString("utf8");
    } else if (Object.prototype.hasOwnProperty.call(value, key)) {
      normalized[key] = value[key];
    }
  }
  return normalized;
}

function normalizeNode(value: Record<string, unknown>): Record<string, unknown> {
  const allowed = new Set(["script", "outcomes", "inputs", "outputs"]);
  rejectUnknown("node", value, allowed, NODE_SERVER_FIELDS);
  return copyAllowed(value, allowed);
}

function normalizeTree(
  value: Record<string, unknown>,
  submitted: Readonly<Record<string, unknown>>
): Record<string, unknown> {
  const allowed = new Set(Object.keys(submitted));
  rejectUnknown("tree", value, allowed, TREE_SERVER_FIELDS);
  const normalized = copyAllowed(value, allowed);
  if (isPlainObject(normalized.nodes) && isPlainObject(submitted.nodes)) {
    const nodes: Record<string, unknown> = {};
    for (const [id, node] of Object.entries(normalized.nodes)) {
      if (!isPlainObject(node)) {
        throw new AicLaneError("tree confirming read contains a non-object node entry");
      }
      const submittedNode = submitted.nodes[id];
      const copy = { ...node };
      if (isPlainObject(submittedNode) && !("version" in submittedNode)) {
        delete copy.version;
      }
      nodes[id] = copy;
    }
    normalized.nodes = nodes;
  }
  return normalized;
}

function rejectUnknown(
  kind: AicResourceKind,
  value: Record<string, unknown>,
  functional: ReadonlySet<string>,
  serverOwned: ReadonlySet<string>
): void {
  const unknown = Object.keys(value).filter(
    (key) => !functional.has(key) && !serverOwned.has(key)
  );
  if (unknown.length > 0) {
    throw new AicLaneError(
      `${kind} confirming read contained unexpected fields: ${unknown.join(", ")}`
    );
  }
}

function copyAllowed(
  value: Record<string, unknown>,
  allowed: ReadonlySet<string>
): Record<string, unknown> {
  return Object.fromEntries(
    Object.entries(value).filter(([key]) => allowed.has(key))
  );
}
