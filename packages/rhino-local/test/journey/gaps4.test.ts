/** Journey emulator gaps, round 4: found by rewriting real journeys; generic trees only. */
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { RhinoRunner } from "../../src/runner.ts";
import { IdmConfig, IdmEmulator } from "../../src/idm/index.ts";
import { JourneyConfig, JourneyEmulator } from "../../src/journey/index.ts";
import type { JourneyConfigInput } from "../../src/journey/index.ts";
import type { JsonValue } from "../../src/protocol.ts";
import { SUCCESS_NODE_ID as SUCCESS, FAILURE_NODE_ID as FAILURE } from "../../src/aic/constants.ts";

let runner: RhinoRunner;
beforeAll(async () => {
  runner = await RhinoRunner.spawn();
}, 60_000);
afterAll(async () => {
  await runner?.close();
});

const b64 = (source: string) => Buffer.from(source).toString("base64");
const script = (source: string) => ({ evaluatorVersion: "2.0", script: b64(source) });
function make(input: JourneyConfigInput) {
  return new JourneyEmulator(
    new JourneyConfig(input),
    runner,
    new IdmEmulator(new IdmConfig({ sync: { mappings: [] }, managed: { objects: [] } }), runner),
    {},
  );
}
const run = (emu: JourneyEmulator, tree: string, body: JsonValue = {}) =>
  emu.request({
    method: "POST",
    path: "authenticate",
    query: { authIndexType: "service", authIndexValue: tree },
    body,
  });
type Wire = {
  authId?: string;
  header?: string;
  description?: string;
  stage?: string;
  callbacks: { type: string; input: { name: string; value: JsonValue }[] }[];
};
const answer = (body: JsonValue, values: Record<number, JsonValue>): JsonValue => {
  const copy = structuredClone(body) as Wire;
  for (const [index, value] of Object.entries(values)) {
    const input = copy.callbacks[Number(index)]?.input[0];
    if (!input) throw new Error(`no input at ${index}`);
    input.value = value;
  }
  return copy as unknown as JsonValue;
};
const types = (body: JsonValue) => (body as Wire).callbacks.map((cb) => cb.type);
const props = (body: JsonValue) => {
  const { header, description, stage } = body as Wire;
  return { header, description, stage };
};
const dump = (body: JsonValue): Record<string, unknown> => {
  const callbacks = (body as unknown as { callbacks: { output: { name: string; value: JsonValue }[] }[] }).callbacks;
  return JSON.parse(String(callbacks[0]?.output.find((f) => f.name === "value")?.value)) as Record<string, unknown>;
};

// Scripts. `dumpOf` is the outer tree's terminal reporter: it sends one HiddenValue callback.
const KEYS = ["username", "password", "outerShared", "outerTransient", "inShared", "inTransient", "sawOuterS", "sawOuterT", "sawUser", "sawPw", "midSawUser"];
const dumpOf = (branch: string) =>
  `var o={branch:'${branch}'};${JSON.stringify(KEYS)}.forEach(function(k){var v=nodeState.get(k);o[k]=v===null?null:String(v);});` +
  `callbacksBuilder.hiddenValueCallback('state',JSON.stringify(o));`;
const OUTER = `nodeState.putShared('outerShared','os');nodeState.putTransient('outerTransient','ot');action.goTo('true');`;
const INNER =
  `nodeState.putShared('sawUser',String(nodeState.get('username')));var p=nodeState.get('password');` +
  `nodeState.putShared('sawPw',p===null?'null':'len='+String(p).length);` +
  `nodeState.putShared('sawOuterS',String(nodeState.get('outerShared')));` +
  `nodeState.putShared('sawOuterT',String(nodeState.get('outerTransient')));` +
  `nodeState.putShared('inShared','is');nodeState.putTransient('inTransient','it');action.goTo('true');`;
const CHECK = `action.goTo(String(nodeState.get('username'))==='right'?'true':'false');`;
const MID = `nodeState.putShared('midSawUser',String(nodeState.get('username')));action.goTo('true');`;

type PageSpec = { id: string; children: string[]; header?: string; description?: string; stage?: string };
const COLLECTORS: Record<string, string> = { u: "UsernameCollectorNode", p: "PasswordCollectorNode" };
function input(spec: {
  pages: PageSpec[];
  trees: JourneyConfigInput["trees"];
  scripts: Record<string, string>;
}): JourneyConfigInput {
  const nodes: Record<string, Record<string, Record<string, unknown>>> = {
    PageNode: {},
    UsernameCollectorNode: { u: {} },
    PasswordCollectorNode: { p: {} },
    ScriptedDecisionNode: {},
    InnerTreeEvaluatorNode: {},
  };
  for (const page of spec.pages)
    nodes.PageNode![page.id] = {
      nodes: page.children.map((c) => ({ _id: c, nodeType: COLLECTORS[c] })),
      pageHeader: page.header ? { en: page.header } : {},
      pageDescription: page.description ? { en: page.description } : {},
      ...(page.stage === undefined ? {} : { stage: page.stage }),
    };
  const scripts: Record<string, { evaluatorVersion: string; script: string }> = {};
  for (const [id, source] of Object.entries(spec.scripts)) {
    scripts[id] = script(source);
    nodes.ScriptedDecisionNode![id] = { script: id, outcomes: ["true", "false"] };
  }
  for (const tree of Object.values(spec.trees))
    for (const [id, node] of Object.entries(tree.nodes))
      if (node.nodeType === "InnerTreeEvaluatorNode") nodes.InnerTreeEvaluatorNode![id] = { tree: id.replace(/^to-/, "") };
  return { realm: "alpha", trees: spec.trees, nodes, scripts };
}
const dumps = {
  dT: { nodeType: "ScriptedDecisionNode", connections: { true: SUCCESS } },
  dF: { nodeType: "ScriptedDecisionNode", connections: { true: SUCCESS } },
};
const SCRIPTS = { dT: dumpOf("T"), dF: dumpOf("F"), outer: OUTER, inner: INNER, check: CHECK, mid: MID };

describe("a Page inside an inner tree (doc 25, 2026-11-25)", () => {
  it("sends the inner page's callbacks and own properties, then resumes inside the inner tree", async () => {
    const emu = make(
      input({
        pages: [{ id: "ip", children: ["u", "p"], header: "IN-H", description: "IN-D", stage: "IN-S" }],
        scripts: SCRIPTS,
        trees: {
          outer: {
            entryNodeId: "outer",
            nodes: {
              outer: { nodeType: "ScriptedDecisionNode", connections: { true: "to-inner" } },
              "to-inner": { nodeType: "InnerTreeEvaluatorNode", connections: { true: "dT", false: "dF" } },
              ...dumps,
            },
          },
          inner: {
            entryNodeId: "ip",
            innerTreeOnly: true,
            nodes: {
              ip: { nodeType: "PageNode", connections: { outcome: "inner" } },
              inner: { nodeType: "ScriptedDecisionNode", connections: { true: SUCCESS } },
            },
          },
        },
      }),
    );
    const first = await run(emu, "outer");
    expect(first.status).toBe(200);
    expect(types(first.body)).toEqual(["NameCallback", "PasswordCallback"]);
    expect(props(first.body)).toEqual({ header: "IN-H", description: "IN-D", stage: "IN-S" });
    const second = await run(emu, "outer", answer(first.body, { 0: "kdemo-user", 1: "kdemo-pw-1234" }));
    const state = dump(second.body);
    // Resumed inside the inner tree (its script ran after the page), and its outcome routed the outer tree.
    expect(state).toMatchObject({ branch: "T", username: "kdemo-user", sawUser: "kdemo-user", sawPw: "len=13" });
    // Shared state crosses both ways; transient state is isolated per tree: the outer transient is not
    // seen inside, the inner (and the password) is gone outside.
    expect(state).toMatchObject({ outerShared: "os", sawOuterS: "os", inShared: "is" });
    expect(state).toMatchObject({ sawOuterT: "null", outerTransient: null, inTransient: null, password: null });
  });

  it("does not leak an outer page's properties into an inner page that has none", async () => {
    const emu = make(
      input({
        pages: [
          { id: "op", children: ["u"], header: "OUT-H", description: "OUT-D", stage: "OUT-S" },
          { id: "ip", children: ["p"] },
        ],
        scripts: SCRIPTS,
        trees: {
          outer: {
            entryNodeId: "op",
            nodes: {
              op: { nodeType: "PageNode", connections: { outcome: "to-inner" } },
              "to-inner": { nodeType: "InnerTreeEvaluatorNode", connections: { true: "dT", false: "dF" } },
              ...dumps,
            },
          },
          inner: {
            entryNodeId: "ip",
            innerTreeOnly: true,
            nodes: {
              ip: { nodeType: "PageNode", connections: { outcome: "inner" } },
              inner: { nodeType: "ScriptedDecisionNode", connections: { true: SUCCESS } },
            },
          },
        },
      }),
    );
    const outerPage = await run(emu, "outer");
    expect(props(outerPage.body)).toEqual({ header: "OUT-H", description: "OUT-D", stage: "OUT-S" });
    const innerPage = await run(emu, "outer", answer(outerPage.body, { 0: "kdemo-outer" }));
    expect(types(innerPage.body)).toEqual(["PasswordCallback"]);
    expect(props(innerPage.body)).toEqual({ header: undefined, description: undefined, stage: undefined });
    const done = await run(emu, "outer", answer(innerPage.body, { 0: "kdemo-pw-1234" }));
    expect(dump(done.body)).toMatchObject({ branch: "T", username: "kdemo-outer", sawUser: "kdemo-outer" });
  });

  // The discriminating case: a wrong answer must send the inner page again, not leave the inner tree
  // (which would route the outer one by `false`) and not resume past the page.
  it("a page visited twice: wrong answers return to the inner page; the right one leaves", async () => {
    const emu = make(
      input({
        pages: [{ id: "ip", children: ["u"], header: "C-H", description: "C-D" }],
        scripts: SCRIPTS,
        trees: {
          outer: {
            entryNodeId: "to-inner",
            nodes: {
              "to-inner": { nodeType: "InnerTreeEvaluatorNode", connections: { true: "dT", false: "dF" } },
              ...dumps,
            },
          },
          inner: {
            entryNodeId: "ip",
            innerTreeOnly: true,
            nodes: {
              ip: { nodeType: "PageNode", connections: { outcome: "check" } },
              check: { nodeType: "ScriptedDecisionNode", connections: { true: SUCCESS, false: "ip" } },
            },
          },
        },
      }),
    );
    let reply = await run(emu, "outer");
    for (const wrong of ["wrong1", "wrong2"]) {
      expect(types(reply.body)).toEqual(["NameCallback"]);
      expect(props(reply.body)).toMatchObject({ header: "C-H", description: "C-D" });
      reply = await run(emu, "outer", answer(reply.body, { 0: wrong }));
    }
    expect(types(reply.body)).toEqual(["NameCallback"]);
    reply = await run(emu, "outer", answer(reply.body, { 0: "right" }));
    expect(dump(reply.body)).toMatchObject({ branch: "T", username: "right" });
  });

  it("an empty submission re-sends the inner page with its properties; a failing inner tree routes the outer by false", async () => {
    const emu = make(
      input({
        pages: [{ id: "ip", children: ["u", "p"], header: "E-H", description: "E-D" }],
        scripts: SCRIPTS,
        trees: {
          outer: {
            entryNodeId: "to-inner",
            nodes: {
              "to-inner": { nodeType: "InnerTreeEvaluatorNode", connections: { true: "dT", false: "dF" } },
              ...dumps,
            },
          },
          inner: {
            entryNodeId: "ip",
            innerTreeOnly: true,
            nodes: {
              ip: { nodeType: "PageNode", connections: { outcome: "check" } },
              check: { nodeType: "ScriptedDecisionNode", connections: { true: SUCCESS, false: FAILURE } },
            },
          },
        },
      }),
    );
    const first = await run(emu, "outer");
    const resent = await run(emu, "outer", answer(first.body, { 0: "", 1: "" }));
    expect(types(resent.body)).toEqual(["NameCallback", "PasswordCallback"]);
    expect(props(resent.body)).toMatchObject({ header: "E-H", description: "E-D" });
    const done = await run(emu, "outer", answer(resent.body, { 0: "nope", 1: "kdemo-pw-1234" }));
    expect(dump(done.body)).toMatchObject({ branch: "F", username: "nope" });
  });

  it("nests two deep: the page in the innermost tree, the shared state seen by the middle one", async () => {
    const emu = make(
      input({
        pages: [{ id: "ip", children: ["u"], header: "D-H" }],
        scripts: SCRIPTS,
        trees: {
          outer: {
            entryNodeId: "to-mid",
            nodes: {
              "to-mid": { nodeType: "InnerTreeEvaluatorNode", connections: { true: "dT", false: "dF" } },
              ...dumps,
            },
          },
          mid: {
            entryNodeId: "to-deep",
            innerTreeOnly: true,
            nodes: {
              "to-deep": { nodeType: "InnerTreeEvaluatorNode", connections: { true: "mid", false: FAILURE } },
              mid: { nodeType: "ScriptedDecisionNode", connections: { true: SUCCESS } },
            },
          },
          deep: {
            entryNodeId: "ip",
            innerTreeOnly: true,
            nodes: {
              ip: { nodeType: "PageNode", connections: { outcome: "inner" } },
              inner: { nodeType: "ScriptedDecisionNode", connections: { true: SUCCESS } },
            },
          },
        },
      }),
    );
    const first = await run(emu, "outer");
    expect(props(first.body)).toMatchObject({ header: "D-H" });
    const done = await run(emu, "outer", answer(first.body, { 0: "kdemo-deep" }));
    expect(dump(done.body)).toMatchObject({ branch: "T", username: "kdemo-deep", sawUser: "kdemo-deep", midSawUser: "kdemo-deep", inShared: "is" });
  });
});

describe("a Page's stage (doc 25, 2026-11-25)", () => {
  const tree = (stage: string | undefined) =>
    make(
      input({
        pages: [{ id: "pg", children: ["u", "p"], ...(stage === undefined ? {} : { stage }) }],
        scripts: {},
        trees: {
          page: {
            entryNodeId: "pg",
            nodes: { pg: { nodeType: "PageNode", connections: { outcome: SUCCESS } } },
          },
        },
      }),
    );
  it("is a top-level stage, repeated when the page is re-sent; empty or absent sends none", async () => {
    for (const [stage, expected] of [["S1", "S1"], ["", undefined], [undefined, undefined]] as const) {
      const emu = tree(stage);
      const first = await run(emu, "page");
      expect((first.body as Wire).stage).toBe(expected);
      const resent = await run(emu, "page", answer(first.body, { 0: "", 1: "" }));
      expect((resent.body as Wire).stage).toBe(expected);
    }
  });
});
