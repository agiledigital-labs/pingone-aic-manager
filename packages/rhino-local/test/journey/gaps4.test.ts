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
const KEYS = ["userName", "oneTimePassword", "oneTimePasswordTimestamp", "pageNodeCallbacks", "username", "password", "outerShared", "outerTransient", "inShared", "inTransient", "sawOuterS", "sawOuterT", "sawUser", "sawPw", "midSawUser"];
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
const COLLECTORS: Record<string, string> = {
  u: "UsernameCollectorNode",
  p: "PasswordCollectorNode",
  o: "OneTimePasswordCollectorDecisionNode",
  c: "ChoiceCollectorNode",
};
function input(spec: {
  pages: PageSpec[];
  trees: JourneyConfigInput["trees"];
  scripts: Record<string, string>;
}): JourneyConfigInput {
  const nodes: Record<string, Record<string, Record<string, unknown>>> = {
    PageNode: {},
    UsernameCollectorNode: { u: {} },
    PasswordCollectorNode: { p: {} },
    OneTimePasswordCollectorDecisionNode: { o: { passwordExpiryTime: 5 } },
    ChoiceCollectorNode: { c: { prompt: "Pick", choices: ["alpha", "beta", "gamma"], defaultChoice: "beta" } },
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
const SEED = `nodeState.putShared('oneTimePassword','123456');nodeState.putShared('oneTimePasswordTimestamp',Math.floor(new Date().getTime()/1000));action.goTo('true');`;
const SCRIPTS = { seed: SEED, dA: dumpOf("alpha"), dB: dumpOf("beta"), dG: dumpOf("gamma"), dT: dumpOf("T"), dF: dumpOf("F"), outer: OUTER, inner: INNER, check: CHECK, mid: MID };

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
  // Review round 2 P2-4: the committed probe answered its stage pages first time, and re-sent only
  // a page with header and description, so the stage's repetition is the emulator's assumption.
  it("measures the stage's presence and assumes its repetition", async () => {
    const emu = tree("S1");
    const first = await run(emu, "page");
    const resent = await run(emu, "page", answer(first.body, { 0: "", 1: "" }));
    const of = (r: typeof first, kind: string) =>
      (r.provenance ?? []).filter((e) => e.kind === kind).map((e) => e.behavior).join("\n");
    expect(of(first, "measured")).toContain("sends its non-empty stage");
    expect(of(first, "measured")).not.toContain("repeats it");
    expect(of(resent, "assumed")).toContain("a re-sent page repeats its stage");
    // Review round 3 P2-4: the re-send itself must not claim the repetition as measured either.
    expect(of(resent, "measured")).not.toMatch(/repeat[^\n]*stage/);
  });
  it("says the callback _id numbering came from an uncommitted run", async () => {
    const first = await run(tree("S1"), "page");
    const id = (first.provenance ?? []).find((e) => e.behavior.includes("top-level numeric _id"));
    expect(id?.behavior).toContain("uncommitted");
  });
});

// E42: OneTimePasswordCollectorDecisionNode and ChoiceCollectorNode as Page children (doc 25, 2026-11-25).
describe("one-time password and choice nodes as Page children", () => {
  const pageTree = (children: string[], connections: Record<string, string>) =>
    make(
      input({
        pages: [{ id: "pg", children }],
        scripts: SCRIPTS,
        trees: {
          page: {
            entryNodeId: "seed",
            nodes: {
              seed: { nodeType: "ScriptedDecisionNode", connections: { true: "pg" } },
              pg: { nodeType: "PageNode", connections },
              dT: dumps.dT,
              dF: dumps.dF,
              dA: dumps.dT,
              dB: dumps.dT,
              dG: dumps.dT,
            },
          },
        },
      }),
    );
  const otp = (children: string[]) => pageTree(children, { true: "dT", false: "dF" });
  const choice = (children: string[]) => pageTree(children, { alpha: "dA", beta: "dB", gamma: "dG" });

  it("a one-time password child: one PasswordCallback; right answer takes true and consumes the password, wrong takes false and keeps it", async () => {
    for (const [answerText, branch, kept] of [["123456", "T", null], ["000000", "F", "123456"]] as const) {
      const emu = otp(["o"]);
      const first = await run(emu, "page");
      expect(types(first.body)).toEqual(["PasswordCallback"]);
      expect((first.body as Wire).callbacks[0]?.input[0]?.name).toBe("IDToken1");
      const done = dump((await run(emu, "page", answer(first.body, { 0: answerText }))).body);
      expect(done).toMatchObject({ branch, oneTimePassword: kept, pageNodeCallbacks: null });
      expect(done.oneTimePasswordTimestamp === null).toBe(kept === null);
    }
  });

  // Review round 1 P2-1: a re-send keeps the lone-child rule (assumed; the probe stops at the first answer).
  it("a lone one-time password child re-sent after an empty answer still leaves no pageNodeCallbacks", async () => {
    const emu = otp(["o"]);
    const first = await run(emu, "page");
    const resent = await run(emu, "page", answer(first.body, { 0: "" }));
    expect(types(resent.body)).toEqual(["PasswordCallback"]);
    const done = dump((await run(emu, "page", answer(resent.body, { 0: "123456" }))).body);
    expect(done).toMatchObject({ branch: "T", pageNodeCallbacks: null });
  });

  // Review round 2 P2-3: the re-send is measured for this node (doc 25's table); the Attribute
  // Collector's echo-and-failedPolicies measurement is another node's wire shape and must not ride it.
  it("a lone one-time password re-send carries its own evidence, not the attribute collector's", async () => {
    const emu = otp(["o"]);
    const first = await run(emu, "page");
    const resent = await run(emu, "page", answer(first.body, { 0: "" }));
    const kinds = (kind: string) =>
      (resent.provenance ?? []).filter((e) => e.kind === kind).map((e) => e.behavior).join("\n");
    expect(kinds("measured")).not.toContain("failedPolicies");
    expect(kinds("measured")).toContain("an empty one-time password answer sends the same page again");
    expect(kinds("assumed")).toContain("leaves no pageNodeCallbacks");
  });

  // Review round 3 P2-3: beside a username child, the page is re-sent for the one-time password child
  // (doc 25's pageU row), so it carries that node's evidence; the attribute collector's never applies.
  it("a mixed page re-sent for its empty one-time password carries the rejecting child's evidence", async () => {
    const emu = otp(["u", "o"]);
    const first = await run(emu, "page");
    const resent = await run(emu, "page", answer(first.body, { 0: "kdemo-user", 1: "" }));
    expect(types(resent.body)).toEqual(["NameCallback", "PasswordCallback"]);
    const kinds = (kind: string) =>
      (resent.provenance ?? []).filter((e) => e.kind === kind).map((e) => e.behavior).join("\n");
    expect(kinds("measured")).not.toContain("failedPolicies");
    expect(kinds("measured")).toContain("an empty one-time password answer sends the same page again");
    expect(kinds("assumed")).toContain("echoes an accepted sibling's submitted values");
  });

  it("after a username collector the one-time password input is IDToken2, and the page records both callbacks", async () => {
    const emu = otp(["u", "o"]);
    const first = await run(emu, "page");
    expect(types(first.body)).toEqual(["NameCallback", "PasswordCallback"]);
    expect((first.body as Wire).callbacks[1]?.input[0]?.name).toBe("IDToken2");
    const done = dump((await run(emu, "page", answer(first.body, { 0: "kdemo-user", 1: "123456" }))).body);
    expect(done).toMatchObject({ branch: "T", username: "kdemo-user" });
    // Present with a collector beside it. This dump String()s it: the measured map reads as one
    // "[object Object]" (the wire array it once was read as two); prereview.test.ts checks content.
    expect(done.pageNodeCallbacks).toBe("[object Object]");
  });

  it("an empty one-time password re-sends the whole page", async () => {
    const emu = otp(["u", "o"]);
    const first = await run(emu, "page");
    const resent = await run(emu, "page", answer(first.body, { 0: "kdemo-user", 1: "" }));
    expect(types(resent.body)).toEqual(["NameCallback", "PasswordCallback"]);
  });

  it("a choice child: default index sent, the answered index names the page outcome, an index outside the list fails", async () => {
    for (const [index, branch] of [[0, "alpha"], [1, "beta"], [2, "gamma"]] as const) {
      const emu = choice(["c"]);
      const first = await run(emu, "page");
      expect(types(first.body)).toEqual(["ChoiceCallback"]);
      expect((first.body as Wire).callbacks[0]?.input[0]?.value).toBe(1);
      const done = dump((await run(emu, "page", answer(first.body, { 0: index }))).body);
      expect(done).toMatchObject({ branch, pageNodeCallbacks: null });
    }
    const emu = choice(["c"]);
    const first = await run(emu, "page");
    expect((await run(emu, "page", answer(first.body, { 0: 7 }))).status).toBe(401);
  });

  it("a choice after a username collector is IDToken2 and records both callbacks", async () => {
    const emu = choice(["u", "c"]);
    const first = await run(emu, "page");
    expect((first.body as Wire).callbacks[1]?.input[0]?.name).toBe("IDToken2");
    const done = dump((await run(emu, "page", answer(first.body, { 0: "kdemo-user", 1: 2 }))).body);
    expect(done).toMatchObject({ branch: "gamma", username: "kdemo-user" });
    // Present with a collector beside it. This dump String()s it: the measured map reads as one
    // "[object Object]" (the wire array it once was read as two); prereview.test.ts checks content.
    expect(done.pageNodeCallbacks).toBe("[object Object]");
  });
});

// E49: Identify Existing User reads shared `username` (doc 25, 2026-11-25).
describe("Identify Existing User's lookup key", () => {
  const A = "00000000-0000-0000-0000-0000000000a1";
  const B = "00000000-0000-0000-0000-0000000000b2";
  async function identify(seed: string) {
    const idm = new IdmEmulator(
      new IdmConfig({
        sync: { mappings: [] },
        managed: { objects: [{ name: "alpha_user", schema: { properties: { userName: { type: "string", title: "Username" } } } }] },
      }),
      runner,
    );
    for (const [id, userName] of [[A, "kdemo-ua"], [B, "kdemo-ub"]] as const)
      await idm.request({ method: "PUT", path: `managed/alpha_user/${id}`, body: { userName } });
    const emu = new JourneyEmulator(
      new JourneyConfig({
        realm: "alpha",
        trees: {
          t: {
            entryNodeId: "seed",
            nodes: {
              seed: { nodeType: "ScriptedDecisionNode", connections: { true: "id" } },
              id: { nodeType: "IdentifyExistingUserNode", connections: { true: "dT", false: "dF" } },
              dT: { nodeType: "ScriptedDecisionNode", connections: { true: SUCCESS } },
              dF: { nodeType: "ScriptedDecisionNode", connections: { true: SUCCESS } },
            },
          },
        },
        nodes: {
          ScriptedDecisionNode: {
            seed: { script: "seed", outcomes: ["true"] },
            dT: { script: "dT", outcomes: ["true"] },
            dF: { script: "dF", outcomes: ["true"] },
          },
          IdentifyExistingUserNode: { id: { identityAttribute: "userName", identifier: "userName" } },
        },
        scripts: {
          seed: script(`${seed} action.goTo('true');`),
          dT: script(dumpOf("T")),
          dF: script(dumpOf("F")),
        },
      }),
      runner,
      idm,
      {},
    );
    return dump((await run(emu, "t")).body);
  }
  it("a shared username is looked up and written back; a shared userName alone is not read", async () => {
    expect(await identify(`nodeState.putShared('username','kdemo-ua');`)).toMatchObject({ branch: "T", username: "kdemo-ua" });
    const camel = await identify(`nodeState.putShared('userName','kdemo-ua');`);
    expect(camel).toMatchObject({ branch: "F", username: null, userName: "kdemo-ua" });
  });
  it("with both set, username decides, whichever user each names", async () => {
    const a = await identify(`nodeState.putShared('username','kdemo-ua');nodeState.putShared('userName','kdemo-ub');`);
    expect(a).toMatchObject({ branch: "T", username: "kdemo-ua" });
    const b = await identify(`nodeState.putShared('username','kdemo-ub');nodeState.putShared('userName','kdemo-ua');`);
    expect(b).toMatchObject({ branch: "T", username: "kdemo-ub" });
  });
  it("a transient username is not read", async () => {
    expect(await identify(`nodeState.putTransient('username','kdemo-ua');`)).toMatchObject({ branch: "F" });
  });
});

// E48: a Create Object on alpha_user without a password, and a journey's view of alpha_user (doc 25, 2026-11-25).
describe("alpha_user in a journey: no password, and no password in any read", () => {
  const WITH = "00000000-0000-0000-0000-0000000000c3";
  const READ = `
    var shape = function (o) { return { has: Object.prototype.hasOwnProperty.call(o, 'password'), inJson: JSON.stringify(o).indexOf('"password"') >= 0 }; };
    var q = openidm.query('managed/alpha_user', { _queryFilter: 'userName eq "kdemo-created"' });
    var c = q.result[0];
    var rows = { created: { read: shape(openidm.read('managed/alpha_user/' + c._id)), star: shape(openidm.read('managed/alpha_user/' + c._id, null, ['*'])),
                            only: shape(openidm.read('managed/alpha_user/' + c._id, null, ['password'])), query: shape(c) },
                 withPw: { read: shape(openidm.read('managed/alpha_user/${WITH}')), query: shape(openidm.query('managed/alpha_user', { _queryFilter: 'userName eq "kdemo-with"' }).result[0]) } };
    callbacksBuilder.hiddenValueCallback('state', JSON.stringify(rows));
  `;
  it("creates the user and reads it back without a password key, as it reads a user that has one", async () => {
    const idm = new IdmEmulator(
      new IdmConfig({
        sync: { mappings: [] },
        managed: {
          objects: [
            { name: "alpha_user", schema: { properties: { userName: { type: "string" }, mail: { type: "string" }, password: { type: "string" } } } },
          ],
        },
      }),
      runner,
    );
    await idm.request({ method: "PUT", path: `managed/alpha_user/${WITH}`, body: { userName: "kdemo-with", mail: "w@example.com", password: "Kdemo-Pw-12345" } });
    const emu = new JourneyEmulator(
      new JourneyConfig({
        realm: "alpha",
        trees: {
          t: {
            entryNodeId: "seed",
            nodes: {
              seed: { nodeType: "ScriptedDecisionNode", connections: { true: "create" } },
              create: { nodeType: "CreateObjectNode", connections: { CREATED: "read", FAILURE: SUCCESS } },
              read: { nodeType: "ScriptedDecisionNode", connections: { true: SUCCESS } },
            },
          },
        },
        nodes: {
          ScriptedDecisionNode: { seed: { script: "seed", outcomes: ["true"] }, read: { script: "read", outcomes: ["true"] } },
          CreateObjectNode: { create: { identityResource: "managed/alpha_user" } },
        },
        scripts: {
          seed: script(`nodeState.putShared('objectAttributes', {userName: 'kdemo-created', mail: 'c@example.com'}); action.goTo('true');`),
          read: script(READ),
        },
      }),
      runner,
      idm,
      {},
    );
    const reply = await run(emu, "t");
    expect(reply.status).toBe(200);
    const rows = dump(reply.body) as Record<string, Record<string, { has: boolean; inJson: boolean }>>;
    for (const group of Object.values(rows))
      for (const shape of Object.values(group)) expect(shape).toEqual({ has: false, inJson: false });
  });
  // Discriminating case: the stored record carries a null password (nothing was supplied), and a later patch of
  // some other field writes the whole record back. A wrong model treats that null as a password write and fails.
  it("patches another field of a user created without a password", async () => {
    const idm = new IdmEmulator(
      new IdmConfig({
        sync: { mappings: [] },
        managed: {
          objects: [
            { name: "alpha_user", schema: { properties: { userName: { type: "string" }, mail: { type: "string" }, sn: { type: "string" }, password: { type: "string" } } } },
          ],
        },
      }),
      runner,
    );
    const emu = new JourneyEmulator(
      new JourneyConfig({
        realm: "alpha",
        trees: {
          t: {
            entryNodeId: "seed",
            nodes: {
              seed: { nodeType: "ScriptedDecisionNode", connections: { true: "create" } },
              create: { nodeType: "CreateObjectNode", connections: { CREATED: "patch", FAILURE: SUCCESS } },
              patch: { nodeType: "ScriptedDecisionNode", connections: { true: SUCCESS } },
            },
          },
        },
        nodes: {
          ScriptedDecisionNode: { seed: { script: "seed", outcomes: ["true"] }, patch: { script: "patch", outcomes: ["true"] } },
          CreateObjectNode: { create: { identityResource: "managed/alpha_user" } },
        },
        scripts: {
          seed: script(`nodeState.putShared('objectAttributes', {userName: 'kdemo-p', mail: 'p@example.com'}); action.goTo('true');`),
          patch: script(`
            var c = openidm.query('managed/alpha_user', { _queryFilter: 'userName eq "kdemo-p"' }).result[0];
            openidm.patch('managed/alpha_user/' + c._id, null, [{ operation: 'replace', field: '/sn', value: 'Patched' }]);
            callbacksBuilder.hiddenValueCallback('state', JSON.stringify({ sn: openidm.read('managed/alpha_user/' + c._id).sn }));
          `),
        },
      }),
      runner,
      idm,
      {},
    );
    const reply = await run(emu, "t");
    expect(reply.status).toBe(200);
    expect(dump(reply.body)).toEqual({ sn: "Patched" });
  });
});
