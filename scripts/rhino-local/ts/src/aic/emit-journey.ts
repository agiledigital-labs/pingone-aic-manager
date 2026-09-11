import { randomUUID } from "node:crypto";
import type { Case } from "../case/types.ts";
import {
  FAILURE_NODE_ID,
  RESOURCE_PREFIX,
  SETUP_OUTCOME,
  SUCCESS_NODE_ID,
} from "./constants.ts";
import { emitResultScript } from "./emit-result.ts";
import { emitSetupScript } from "./emit-setup.ts";

export interface EmitJourneyOptions {
  /** Short token used in tree/script names. Must match `[A-Za-z0-9-]{1,32}`. */
  runId?: string;
  realm?: string;
  idFactory?: () => string;
}

export interface WrapperInvoke {
  headers: Record<string, string[]>;
  parameters: Record<string, string[]>;
  cookies: Record<string, string>;
}

export interface ScriptResource {
  id: string;
  name: string;
  source: string;
  role: "setup" | "subject" | "result";
  /** Baked-in outcome, only for `role: "result"`. */
  outcome?: string;
}

export interface NodeResource {
  id: string;
  displayName: string;
  scriptId: string;
  outcomes: string[];
  connections: Record<string, string>;
  x: number;
  y: number;
}

export interface WrapperJourney {
  treeName: string;
  realm: string;
  identityResource: string;
  scripts: ScriptResource[];
  nodes: NodeResource[];
  /** Tree PUT body. No `_id` / `_rev`. */
  treeBody: Record<string, unknown>;
  /** Node PUT bodies keyed by node id. */
  nodeBodies: Record<string, Record<string, unknown>>;
  subjectOutcomes: string[];
  invoke: WrapperInvoke;
}

const RUN_ID_PATTERN = /^[A-Za-z0-9-]{1,32}$/;

/**
 * Outcomes declared on the subject node. Always includes `true` and `false`
 * plus `expect.outcome`, so a script that takes the other branch still reaches
 * a result node that can dump state.
 */
export function subjectOutcomes(kase: Case): string[] {
  const set = new Set<string>(["true", "false"]);
  set.add(kase.expect.outcome);
  return [...set];
}

export function emitWrapperJourney(
  kase: Case,
  source: string,
  options: EmitJourneyOptions = {}
): WrapperJourney {
  if (kase.given.engine === "legacy") {
    throw new Error(
      "rhino-local: AIC wrapper emit is next-gen only; legacy needs JavaImporter + Action.send"
    );
  }
  const runId = options.runId ?? randomUUID().replace(/-/g, "").slice(0, 12);
  if (!RUN_ID_PATTERN.test(runId)) {
    throw new Error(
      `rhino-local: runId ${JSON.stringify(runId)} must match ${RUN_ID_PATTERN}`
    );
  }
  const realm = options.realm ?? kase.given.realm ?? "alpha";
  const nextId = options.idFactory ?? randomUUID;
  const outcomes = subjectOutcomes(kase);
  const treeName = `${RESOURCE_PREFIX}-${runId}`;

  const setupScript: ScriptResource = {
    id: nextId(),
    name: `${treeName}-setup`,
    source: emitSetupScript(kase.given),
    role: "setup",
  };
  const subjectScript: ScriptResource = {
    id: nextId(),
    name: `${treeName}-subject`,
    source,
    role: "subject",
  };

  const resultScripts: ScriptResource[] = [];
  const resultNodes: NodeResource[] = [];
  const resultByOutcome = new Map<string, NodeResource>();
  for (let index = 0; index < outcomes.length; index += 1) {
    const outcome = outcomes[index];
    if (outcome === undefined) {
      continue;
    }
    const script: ScriptResource = {
      id: nextId(),
      name: `${treeName}-result-${slug(outcome)}`,
      source: emitResultScript(outcome),
      role: "result",
      outcome,
    };
    resultScripts.push(script);
    const node: NodeResource = {
      id: nextId(),
      displayName: `result ${outcome}`,
      scriptId: script.id,
      outcomes: [SETUP_OUTCOME],
      connections: { [SETUP_OUTCOME]: SUCCESS_NODE_ID },
      x: 560,
      y: 80 + index * 140,
    };
    resultNodes.push(node);
    resultByOutcome.set(outcome, node);
  }

  const subjectConnections: Record<string, string> = {};
  for (const outcome of outcomes) {
    const resultNode = resultByOutcome.get(outcome);
    if (resultNode === undefined) {
      continue;
    }
    subjectConnections[outcome] = resultNode.id;
  }

  const subjectNode: NodeResource = {
    id: nextId(),
    displayName: kase.name,
    scriptId: subjectScript.id,
    outcomes,
    connections: subjectConnections,
    x: 320,
    y: 200,
  };
  const setupNode: NodeResource = {
    id: nextId(),
    displayName: "setup",
    scriptId: setupScript.id,
    outcomes: [SETUP_OUTCOME],
    connections: { [SETUP_OUTCOME]: subjectNode.id },
    x: 80,
    y: 200,
  };

  const nodes = [setupNode, subjectNode, ...resultNodes];
  const scripts = [setupScript, subjectScript, ...resultScripts];
  const treeNodes: Record<string, unknown> = {};
  for (const node of nodes) {
    treeNodes[node.id] = {
      connections: node.connections,
      displayName: node.displayName,
      nodeType: "ScriptedDecisionNode",
      version: "1.0",
      x: node.x,
      y: node.y,
    };
  }

  const treeBody: Record<string, unknown> = {
    identityResource: `managed/${realm}_user`,
    entryNodeId: setupNode.id,
    innerTreeOnly: false,
    description: "rhino-local AIC lane throwaway. Safe to delete.",
    noSession: false,
    mustRun: false,
    enabled: true,
    transactionalOnly: false,
    uiConfig: { categories: '["Test"]' },
    nodes: treeNodes,
  };

  const nodeBodies: Record<string, Record<string, unknown>> = {};
  for (const node of nodes) {
    nodeBodies[node.id] = scriptedDecisionBody(node);
  }

  return {
    treeName,
    realm,
    identityResource: `managed/${realm}_user`,
    scripts,
    nodes,
    treeBody,
    nodeBodies,
    subjectOutcomes: outcomes,
    invoke: {
      headers: copyStringArrayMap(kase.given.requestHeaders),
      parameters: copyStringArrayMap(kase.given.requestParameters),
      cookies: copyStringMap(kase.given.requestCookies),
    },
  };
}

function scriptedDecisionBody(node: NodeResource): Record<string, unknown> {
  return {
    _id: node.id,
    _type: {
      _id: "ScriptedDecisionNode",
      collection: true,
      name: "Scripted Decision",
    },
    _outcomes: node.outcomes.map((id) => ({ id, displayName: id })),
    inputs: ["*"],
    outputs: ["*"],
    outcomes: node.outcomes,
    script: node.scriptId,
  };
}

function slug(outcome: string): string {
  const cleaned = outcome.replace(/[^A-Za-z0-9]+/g, "-").replace(/^-|-$/g, "");
  return cleaned.length > 0 ? cleaned : "outcome";
}

function copyStringArrayMap(
  value: Record<string, string[]> | undefined
): Record<string, string[]> {
  if (value === undefined) {
    return {};
  }
  const copy: Record<string, string[]> = {};
  for (const [key, items] of Object.entries(value)) {
    copy[key] = items.slice();
  }
  return copy;
}

function copyStringMap(
  value: Record<string, string> | undefined
): Record<string, string> {
  if (value === undefined) {
    return {};
  }
  return { ...value };
}

export const STATIC_NODE_IDS = {
  success: SUCCESS_NODE_ID,
  failure: FAILURE_NODE_ID,
} as const;
