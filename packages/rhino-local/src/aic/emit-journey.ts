import { randomUUID } from "node:crypto";
import type { Case } from "../case/types.ts";
import {
  FAILURE_NODE_ID,
  RESOURCE_PREFIX,
  SETUP_OUTCOME,
  SUCCESS_NODE_ID,
} from "./constants.ts";
import { emitResultScript } from "./emit-result.ts";
import { instrumentSubject } from "./emit-subject.ts";
import type { LeaseIdentity } from "./lease-identity.ts";

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
  role: "subject" | "result";
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

export interface LeasedJourneyOptions {
  identity: LeaseIdentity;
  realm: string;
  suiteName: string;
}

const RUN_ID_PATTERN = /^[A-Za-z0-9-]{1,32}$/;

/**
 * Outcomes declared on the subject node. Always includes `true` and `false`
 * plus `expect.outcome` and everything `case.outcomes` declares, so a script
 * that takes the other branch still reaches a result node that can dump state.
 * A pass expected to suspend has no outcome to add, which is why the declared
 * vocabulary is folded in as well — otherwise a step chain would wire up only
 * the outcomes its first pass mentions.
 */
export function subjectOutcomes(kase: Case): string[] {
  const set = new Set<string>(["true", "false"]);
  if (kase.expect.outcome !== null) {
    set.add(kase.expect.outcome);
  }
  for (const outcome of kase.outcomes ?? []) {
    set.add(outcome);
  }
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
  const instrumented = instrumentSubject(source, runId, kase.given);

  const subjectScript: ScriptResource = {
    id: nextId(),
    name: `${treeName}-subject`,
    source: instrumented.source,
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
      source: emitResultScript(outcome, instrumented.snapshotKey),
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
    x: 80,
    y: 200,
  };

  const nodes = [subjectNode, ...resultNodes];
  const scripts = [subjectScript, ...resultScripts];
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
    entryNodeId: subjectNode.id,
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

/** Emit the fixed graph owned by one file lease. */
export function emitLeasedJourney(options: LeasedJourneyOptions): WrapperJourney {
  const { identity } = options;
  const inertSource = [
    `// ${identity.marker}`,
    "// Inert until the owning file lease arms this subject slot.",
    'throw new Error("rhino-local AIC subject invoked before it was armed");',
    "",
  ].join("\n");
  const subjectScript: ScriptResource = {
    id: identity.ids.subjectScript,
    name: `${identity.treeName}-subject`,
    source: inertSource,
    role: "subject",
  };
  const resultScripts: ScriptResource[] = identity.outcomes.map((outcome) => ({
    id: identity.ids.resultScripts[outcome] as string,
    name: `${identity.treeName}-result-${slug(outcome)}`,
    source: `${`// ${identity.marker}\n`}${emitResultScript(
      outcome,
      identity.snapshotKey,
      identity.structuralDigest
    )}`,
    role: "result",
    outcome,
  }));
  const resultNodes: NodeResource[] = identity.outcomes.map((outcome, index) => ({
    id: identity.ids.resultNodes[outcome] as string,
    displayName: `result ${outcome}`,
    scriptId: identity.ids.resultScripts[outcome] as string,
    outcomes: [SETUP_OUTCOME],
    connections: { [SETUP_OUTCOME]: SUCCESS_NODE_ID },
    x: 560,
    y: 80 + index * 140,
  }));
  const subjectConnections = Object.fromEntries(
    identity.outcomes.map((outcome) => [
      outcome,
      identity.ids.resultNodes[outcome] as string,
    ])
  );
  const subjectNode: NodeResource = {
    id: identity.ids.subjectNode,
    displayName: options.suiteName,
    scriptId: subjectScript.id,
    outcomes: [...identity.outcomes],
    connections: subjectConnections,
    x: 80,
    y: 200,
  };
  const nodes = [subjectNode, ...resultNodes];
  const treeNodes = Object.fromEntries(
    nodes.map((node) => [
      node.id,
      {
        connections: node.connections,
        displayName: node.displayName,
        nodeType: "ScriptedDecisionNode",
        x: node.x,
        y: node.y,
      },
    ])
  );
  const nodeBodies = Object.fromEntries(
    nodes.map((node) => [node.id, leasedScriptedDecisionBody(node)])
  );
  return {
    treeName: identity.treeName,
    realm: options.realm,
    identityResource: `managed/${options.realm}_user`,
    scripts: [subjectScript, ...resultScripts],
    nodes,
    treeBody: {
      identityResource: `managed/${options.realm}_user`,
      entryNodeId: subjectNode.id,
      description: identity.marker,
      enabled: true,
      uiConfig: { categories: '["Test"]' },
      nodes: treeNodes,
    },
    nodeBodies,
    subjectOutcomes: [...identity.outcomes],
    invoke: { headers: {}, parameters: {}, cookies: {} },
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

function leasedScriptedDecisionBody(node: NodeResource): Record<string, unknown> {
  return {
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
