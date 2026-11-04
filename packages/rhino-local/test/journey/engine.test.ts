import { afterAll, beforeAll, expect, it } from "vitest";
import { RhinoRunner } from "../../src/runner.ts";
import { IdmConfig, IdmEmulator } from "../../src/idm/index.ts";
import { JourneyConfig, JourneyEmulator, type JourneyOptions } from "../../src/journey/index.ts";
import { listOutcomes } from "../../src/journey/engine.ts";
import type { JsonValue } from "../../src/protocol.ts";
import { SUCCESS_NODE_ID as SUCCESS, FAILURE_NODE_ID as FAILURE } from "../../src/aic/constants.ts";
import { fillCallbackInputs, parseAuthenticateCallbacks } from "../../src/aic/callbacks.ts";
import { expectEmulationFailure } from "../../src/harness/emulation-guard.ts";
let runner: RhinoRunner;
beforeAll(async () => {
  runner = await RhinoRunner.spawn();
}, 60_000);
afterAll(async () => {
  await runner?.close();
});
const script = (source: string) => ({
  name: "invented-script",
  evaluatorVersion: "2.0",
  language: "JAVASCRIPT",
  script: Buffer.from(source).toString("base64"),
});
function make(
  source: string,
  outcomes = ["true", "false"],
  connections = { true: SUCCESS, false: FAILURE },
  options: JourneyOptions = {},
) {
  return new JourneyEmulator(
    new JourneyConfig({
      realm: "alpha",
      trees: {
        invented: {
          entryNodeId: "one",
          nodes: { one: { nodeType: "ScriptedDecisionNode", connections } },
        },
      },
      nodes: { ScriptedDecisionNode: { one: { script: "script", outcomes } } },
      scripts: { script: script(source) },
    }),
    runner,
    new IdmEmulator(new IdmConfig({ managed: { objects: [] } }), runner),
    options,
  );
}
function request(emu: JourneyEmulator, body = {}) {
  return emu.request({
    method: "POST",
    path: "authenticate",
    query: { authIndexType: "service", authIndexValue: "invented" },
    body,
  });
}
it("success uses the authenticate envelope", async () => {
  const result = await request(
    make("nodeState.putShared('username','invented');action.goTo('true');"),
  );
  expect(result).toMatchObject({
    status: 200,
    body: { tokenId: expect.any(String), successUrl: "/enduser/?realm=/alpha", realm: "/alpha" },
  });
});
it.each(["action.goTo('false');", "action.goTo('nope');", "throw new Error('private detail');"])(
  "tenant failure is 401: %s",
  async (source) => {
    expect(await request(make(source))).toMatchObject({
      status: 401,
      body: { code: 401, reason: "Unauthorized", message: "Login failure" },
    });
  },
);
it.each([
  "action.goTo('false').withErrorMessage('specific');",
  "action.withErrorMessage('specific').goTo('false');",
])("withErrorMessage order: %s", async (source) => {
  expect(await request(make(source))).toMatchObject({ status: 401, body: { message: "specific" } });
});
it("lockout text has no visible effect", async () => {
  expect(
    await request(make("action.goTo('false').withLockoutMessage('not visible');")),
  ).toMatchObject({ status: 401, body: { message: "Login failure" } });
});
it("unwired declared outcome differs from undeclared and missing username", async () => {
  const unwired = await request(
    make("action.goTo('other');", ["other"], { true: SUCCESS, false: FAILURE }),
  );
  expect(unwired).toMatchObject({ status: 500, rawBody: "", body: null });
  expect(await request(make("action.goTo('other');"))).toMatchObject({ status: 401 });
  expect(await request(make("action.goTo('true');"))).toMatchObject({
    status: 500,
    rawBody: "",
    body: null,
  });
});
it("callbacks win over goTo, then the node reruns on resume", async () => {
  const emu = make(
    "nodeState.putShared('passes',(nodeState.get('passes')||0)+1);if(callbacks.isEmpty()){callbacksBuilder.nameCallback('Name');action.goTo('false');}else{nodeState.putShared('username',callbacks.getNameCallbacks().get(0));action.goTo(nodeState.get('passes')===2?'true':'false');}",
  );
  const first = await request(emu);
  expect(first.status).toBe(200);
  expect(first.provenance).toContainEqual({
    kind: "assumed",
    behavior:
      "basic callback wire envelopes reuse binding effect fields; input defaults use empty strings except HiddenValue id and TextInput defaultText",
  });
  expect(parseAuthenticateCallbacks(first.body).callbacks).toEqual([
    { type: "NameCallback", prompt: "Name" },
  ]);
  const resumed = await request(
    emu,
    JSON.parse(
      fillCallbackInputs(first.body, [{ type: "NameCallback", value: "invented" }], "invented"),
    ),
  );
  expect(resumed).toMatchObject({ status: 200, body: { tokenId: expect.any(String) } });
});
it("tampered authId is 400; authId-less callbacks reach the first node", async () => {
  const emu = make(
    "if(callbacks.isEmpty()){callbacksBuilder.nameCallback('Name');}else{nodeState.putShared('username',callbacks.getNameCallbacks().get(0));action.goTo('true');}",
  );
  const first = await request(emu);
  const posted = JSON.parse(
    fillCallbackInputs(first.body, [{ type: "NameCallback", value: "invented" }], "invented"),
  );
  const changed = { ...posted, authId: posted.authId + "tampered" };
  expect(await request(emu, changed)).toMatchObject({
    status: 400,
    body: { message: "Authentication Error: Invalid AuthId Signature" },
  });
  delete posted.authId;
  expect(await request(emu, posted)).toMatchObject({
    status: 200,
    body: { tokenId: expect.any(String) },
  });
});
it("unmocked HTTP cannot be caught, even by finally returning", async () => {
  expectEmulationFailure("unmocked httpClient.send: https://service.example.com/check");
  const emu = make(
    "(function(){try{httpClient.send('https://service.example.com/check').get();}catch(e){}finally{return 42;}})();nodeState.putShared('username','invented');action.goTo('true');",
  );
  expect(await request(emu)).toMatchObject({
    status: 501,
    emulation: {
      kind: "missing",
      missing: ["unmocked httpClient.send: https://service.example.com/check"],
    },
  });
});
it("mocked HTTP goes through the host exactly once including both replay", async () => {
  let calls = 0;
  const emu = make(
    "var r=httpClient.send('https://service.example.com/check').get();if(r.ok&&r.json().allowed){nodeState.putShared('username','invented');action.goTo('true');}else{action.goTo('false');}",
    undefined,
    undefined,
    {
      http: () => {
        calls++;
        return { status: 200, body: { allowed: true } };
      },
    },
  );
  expect(await request(emu)).toMatchObject({ status: 200, body: { tokenId: expect.any(String) } });
  expect(calls).toBe(1);
});
it("script openidm creates through the shared IDM core and executes hooks", async () => {
  const config = new JourneyConfig({
    realm: "alpha",
    trees: {
      invented: {
        entryNodeId: "one",
        nodes: { one: { nodeType: "ScriptedDecisionNode", connections: { true: SUCCESS } } },
      },
    },
    nodes: { ScriptedDecisionNode: { one: { script: "script", outcomes: ["true"] } } },
    scripts: {
      script: script(
        "openidm.create('managed/widget','record',{text:'input'});nodeState.putShared('username','invented');action.goTo('true');",
      ),
    },
  });
  const idm = new IdmEmulator(
    new IdmConfig({
      managed: {
        objects: [
          {
            name: "widget",
            schema: { properties: { text: { type: "string" } } },
            onCreate: { type: "text/javascript", source: "object.text += '-hook';" },
          },
        ],
      },
    }),
    runner,
  );
  expect(await request(new JourneyEmulator(config, runner, idm))).toMatchObject({ status: 200 });
  expect((await idm.request({ method: "GET", path: "managed/widget/record" })).body).toMatchObject({
    text: "input-hook",
  });
});

function graph(
  trees: Record<
    string,
    {
      entryNodeId: string;
      innerTreeOnly?: boolean;
      nodes: Record<string, { nodeType: string; connections: Record<string, string> }>;
    }
  >,
  sources: Record<string, string>,
  innerNodes: Record<string, { tree: string; displayErrorOutcome?: boolean }> = {},
  options: JourneyOptions = {},
) {
  const scripted = Object.fromEntries(
    Object.keys(sources).map((id) => [id, { script: id, outcomes: ["true", "false", "next"] }]),
  );
  return new JourneyEmulator(
    new JourneyConfig({
      realm: "alpha",
      trees,
      nodes: { ScriptedDecisionNode: scripted, InnerTreeEvaluatorNode: innerNodes },
      scripts: Object.fromEntries(
        Object.entries(sources).map(([id, source]) => [id, script(source)]),
      ),
    }),
    runner,
    new IdmEmulator(new IdmConfig({ managed: { objects: [] } }), runner),
    options,
  );
}
function dump(body: unknown): unknown {
  const cb = parseAuthenticateCallbacks(body).callbacks[0];
  return JSON.parse(String(cb?.value));
}
it("transient survives a callback-free edge and disappears on the next round trip, shared survives both", async () => {
  const emu = graph(
    {
      invented: {
        entryNodeId: "seed",
        nodes: {
          seed: { nodeType: "ScriptedDecisionNode", connections: { next: "dump" } },
          dump: { nodeType: "ScriptedDecisionNode", connections: {} },
        },
      },
    },
    {
      seed: "nodeState.remove('maxAuthenticationSessionDuration');nodeState.putShared('maxAuthenticationSessionDuration','caller-owned');nodeState.putTransient('temporary','t');nodeState.putShared('lasting','s');action.goTo('next');",
      dump: "callbacksBuilder.hiddenValueCallback('state',JSON.stringify({ambientName:nodeState.get('maxAuthenticationSessionDuration'),temporary:nodeState.get('temporary'),lasting:nodeState.get('lasting'),hasTransient:nodeState.keys().contains('temporary'),getShared:typeof nodeState.getShared,getTransient:typeof nodeState.getTransient}));",
    },
  );
  const first = await request(emu);
  expect(dump(first.body)).toEqual({
    ambientName: "caller-owned",
    temporary: "t",
    lasting: "s",
    hasTransient: true,
    getShared: "undefined",
    getTransient: "undefined",
  });
  const next = await request(emu, first.body as object);
  expect(dump(next.body)).toEqual({
    ambientName: "caller-owned",
    temporary: null,
    lasting: "s",
    hasTransient: false,
    getShared: "undefined",
    getTransient: "undefined",
  });
});
it.each(["true", "false"])(
  "inner %s maps back to the evaluator and preserves the four visibility boundaries",
  async (outcome) => {
    const emu = graph(
      {
        invented: {
          entryNodeId: "outer",
          nodes: {
            outer: { nodeType: "ScriptedDecisionNode", connections: { next: "inner" } },
            inner: {
              nodeType: "InnerTreeEvaluatorNode",
              connections: { true: "yes", false: "no" },
            },
            yes: { nodeType: "ScriptedDecisionNode", connections: {} },
            no: { nodeType: "ScriptedDecisionNode", connections: {} },
          },
        },
        child: {
          entryNodeId: "child1",
          innerTreeOnly: true,
          nodes: {
            child1: { nodeType: "ScriptedDecisionNode", connections: { next: "child2" } },
            child2: {
              nodeType: "ScriptedDecisionNode",
              connections: { true: SUCCESS, false: FAILURE },
            },
          },
        },
      },
      {
        outer:
          "nodeState.putShared('outerShared','os');nodeState.putTransient('outerTransient','ot');nodeState.putShared('outerName',journey.name());nodeState.putShared('outerFlag',journey.innerJourney());action.goTo('next');",
        child1:
          "nodeState.putShared('sawShared',nodeState.get('outerShared'));nodeState.putShared('sawTransient',nodeState.get('outerTransient'));nodeState.putShared('innerName',journey.name());nodeState.putShared('innerFlag',journey.innerJourney());nodeState.putShared('innerShared','is');nodeState.putTransient('innerTransient','it');action.goTo('next');",
        child2: `nodeState.putShared('innerControl',nodeState.get('innerTransient'));action.goTo('${outcome}');`,
        yes: "callbacksBuilder.hiddenValueCallback('state',JSON.stringify({branch:'true',s:nodeState.get('sawShared'),t:nodeState.get('sawTransient'),inner:nodeState.get('innerShared'),innerT:nodeState.get('innerTransient'),outerT:nodeState.get('outerTransient'),control:nodeState.get('innerControl'),innerName:nodeState.get('innerName'),innerFlag:nodeState.get('innerFlag'),name:journey.name(),flag:journey.innerJourney(),outerName:nodeState.get('outerName'),outerFlag:nodeState.get('outerFlag')}));",
        no: "callbacksBuilder.hiddenValueCallback('state',JSON.stringify({branch:'false',s:nodeState.get('sawShared'),t:nodeState.get('sawTransient'),inner:nodeState.get('innerShared'),innerT:nodeState.get('innerTransient'),outerT:nodeState.get('outerTransient'),control:nodeState.get('innerControl'),innerName:nodeState.get('innerName'),innerFlag:nodeState.get('innerFlag'),name:journey.name(),flag:journey.innerJourney(),outerName:nodeState.get('outerName'),outerFlag:nodeState.get('outerFlag')}));",
      },
      { inner: { tree: "child", displayErrorOutcome: true } },
    );
    const result = await request(emu);
    expect(result.status).toBe(200);
    expect(dump(result.body)).toEqual({
      branch: outcome,
      s: "os",
      t: null,
      inner: "is",
      innerT: null,
      outerT: "ot",
      control: "it",
      innerName: "child",
      innerFlag: true,
      name: "invented",
      flag: false,
      outerName: "invented",
      outerFlag: false,
    });
    const resumed = await request(emu, result.body as object);
    expect(dump(resumed.body)).toMatchObject({
      inner: "is",
      outerT: null,
      innerT: null,
      s: "os",
      control: "it",
    });
    expect(
      await emu.request({
        method: "POST",
        path: "authenticate",
        query: { authIndexType: "service", authIndexValue: "child" },
      }),
    ).toMatchObject({ status: 400, body: { message: "No Configuration found" } });
  },
);
it("stale callback snapshot replays from its immutable shared state and survives a fork", async () => {
  const emu = make(
    "nodeState.putShared('count',(nodeState.get('count')||0)+1);callbacksBuilder.hiddenValueCallback('count',String(nodeState.get('count')));",
  );
  const first = await request(emu);
  expect(dump(first.body)).toBe(1);
  expect(dump((await request(emu, first.body as object)).body)).toBe(2);
  expect(dump((await request(emu, first.body as object)).body)).toBe(2);
  expect(dump((await request(emu.fork(), first.body as object)).body)).toBe(2);
});
it.each(["throw new Error('inner');", "callbacksBuilder.nameCallback('inner');"])(
  "unmeasured inner behavior is missing: %s",
  async (source) => {
    const cause = source.startsWith("throw")
      ? "inner tree script exception"
      : "inner tree callbacks";
    expectEmulationFailure(cause);
    const emu = graph(
      {
        invented: {
          entryNodeId: "inner",
          nodes: {
            inner: {
              nodeType: "InnerTreeEvaluatorNode",
              connections: { true: SUCCESS, false: FAILURE },
            },
          },
        },
        child: {
          entryNodeId: "child",
          innerTreeOnly: true,
          nodes: { child: { nodeType: "ScriptedDecisionNode", connections: {} } },
        },
      },
      { child: source },
      { inner: { tree: "child" } },
    );
    expect(await request(emu)).toMatchObject({ status: 501, emulation: { missing: [cause] } });
  },
);
it("unsupported node requires an explicit declared outcome, validated when the type is known", async () => {
  expectEmulationFailure("node type: InventedNode");
  const input = {
    realm: "alpha",
    trees: {
      invented: {
        entryNodeId: "one",
        nodes: { one: { nodeType: "InventedNode", connections: { next: FAILURE } } },
      },
    },
    nodes: { InventedNode: { one: {} } },
    scripts: {},
  };
  const idm = new IdmEmulator(new IdmConfig({ managed: { objects: [] } }), runner);
  expect(await request(new JourneyEmulator(new JourneyConfig(input), runner, idm))).toMatchObject({
    status: 501,
    emulation: { missing: ["node type: InventedNode"] },
  });
  const declared = await request(
    new JourneyEmulator(new JourneyConfig(input), runner, idm, {
      declaredOutcomes: { one: "next" },
    }),
  );
  expect(declared).toMatchObject({ status: 401 });
  expect(declared.provenance).toContainEqual({
    kind: "assumed",
    behavior: "declared by test: invented/one -> next",
  });
  const known = structuredClone(input);
  known.trees.invented.nodes.one.nodeType = "ValidatedUsernameNode";
  expect(
    await request(
      new JourneyEmulator(
        new JourneyConfig({ ...known, nodes: { ValidatedUsernameNode: { one: {} } } }),
        runner,
        idm,
        { declaredOutcomes: { one: "nope" } },
      ),
    ),
  ).toMatchObject({ status: 401, body: { message: "Login failure" } });
});
it("missing binding use cannot be swallowed in script code", async () => {
  expectEmulationFailure("action.putSessionProperty");
  const result = await request(
    make(
      "try{action.putSessionProperty('secret','value');}catch(e){}nodeState.putShared('username','invented');action.goTo('true');",
    ),
  );
  expect(result).toMatchObject({
    status: 501,
    emulation: { missing: ["action.putSessionProperty"] },
  });
});
it.each([
  "UsernameCollectorNode",
  "PasswordCollectorNode",
  "AttributeCollectorNode",
  "ValidatedPasswordNode",
  "IdentifyExistingUserNode",
  "CreateObjectNode",
  "PatchObjectNode",
  "PageNode",
])("an emulated %s refuses any declared outcome at construction", (type) => {
  const config = new JourneyConfig({
    realm: "alpha",
    trees: {
      invented: { entryNodeId: "node", nodes: { node: { nodeType: type, connections: {} } } },
    },
    nodes: { [type]: { node: {} } },
    scripts: {},
  });
  const idm = new IdmEmulator(new IdmConfig({ managed: { objects: [] } }), runner);
  expect(
    () => new JourneyEmulator(config, runner, idm, { declaredOutcomes: { node: "outcome" } }),
  ).toThrow("declared outcome for emulated node");
});
it("declared ValidatedUsernameNode outcome is judged by its measured vocabulary", async () => {
  const config = new JourneyConfig({
    realm: "alpha",
    trees: {
      invented: {
        entryNodeId: "node",
        nodes: { node: { nodeType: "ValidatedUsernameNode", connections: {} } },
      },
    },
    nodes: { ValidatedUsernameNode: { node: {} } },
    scripts: {},
  });
  const idm = new IdmEmulator(new IdmConfig({ managed: { objects: [] } }), runner);
  // In the vocabulary but unwired: empty 500. Outside it: 401.
  expect(
    await request(
      new JourneyEmulator(config, runner, idm, { declaredOutcomes: { node: "outcome" } }),
    ),
  ).toMatchObject({ status: 500, rawBody: "" });
  expect(
    await request(new JourneyEmulator(config, runner, idm, { declaredOutcomes: { node: "true" } })),
  ).toMatchObject({ status: 401 });
});
// Doc 25 "Declared outcomes": listOutcomes per type, with a node config body.
const stored = new JourneyConfig({
  realm: "alpha",
  trees: {},
  nodes: {
    ScriptedDecisionNode: {
      two: { script: "unused", outcomes: ["a", "b"] },
      one: { script: "unused", outcomes: ["true"] },
    },
    PasswordCollectorNode: { pass: {} },
  },
  scripts: {},
});
it.each([
  ["UsernameCollectorNode", {}, ["outcome"]],
  ["PasswordCollectorNode", {}, ["outcome"]],
  ["AttributeCollectorNode", {}, ["outcome"]],
  ["ValidatedUsernameNode", {}, ["outcome"]],
  ["ValidatedPasswordNode", {}, ["outcome"]],
  ["ScriptedDecisionNode", {}, []],
  ["ScriptedDecisionNode", { outcomes: ["x", "y"] }, ["x", "y"]],
  ["PageNode", {}, []],
  ["PageNode", { nodes: [{ _id: "two", nodeType: "ScriptedDecisionNode" }] }, ["a", "b"]],
  [
    "PageNode",
    {
      nodes: [
        { _id: "one", nodeType: "ScriptedDecisionNode" },
        { _id: "pass", nodeType: "PasswordCollectorNode" },
      ],
    },
    ["outcome"],
  ],
  [
    "PageNode",
    {
      nodes: [
        { _id: "pass", nodeType: "PasswordCollectorNode" },
        { _id: "one", nodeType: "ScriptedDecisionNode" },
      ],
    },
    ["true"],
  ],
  ["InnerTreeEvaluatorNode", { displayErrorOutcome: false }, ["true", "false"]],
  ["InnerTreeEvaluatorNode", { displayErrorOutcome: true }, ["true", "false", "error"]],
  ["IdentifyExistingUserNode", {}, ["true", "false"]],
  ["CreateObjectNode", {}, ["CREATED", "FAILURE"]],
  ["PatchObjectNode", {}, ["PATCHED", "FAILURE"]],
  ["InventedNode", {}, undefined],
] as const)("listOutcomes %s %j is %j", (type, body, expected) => {
  const outcomes = listOutcomes(stored, type, structuredClone(body) as never, (name) => {
    throw new Error(name);
  });
  expect(outcomes === undefined ? undefined : [...outcomes]).toEqual(expected);
});
it("existingSession is undefined without a cookie and a flat caller-supplied map with one", async () => {
  const emu = make(
    "callbacksBuilder.hiddenValueCallback('session',JSON.stringify({kind:typeof existingSession,user:typeof existingSession==='undefined'?null:existingSession.UserId,unknown:typeof existingSession==='undefined'?null:typeof existingSession.notAKey}));",
    undefined,
    undefined,
    { cookieName: "session", sessions: { issued: { UserId: "invented", AuthLevel: "0" } } },
  );
  expect(dump((await request(emu)).body)).toEqual({ kind: "undefined", user: null, unknown: null });
  const result = await emu.request({
    method: "POST",
    path: "authenticate",
    query: { authIndexType: "service", authIndexValue: "invented" },
    cookies: { session: "issued" },
  });
  expect(dump(result.body)).toEqual({ kind: "object", user: "invented", unknown: "undefined" });
});
it.each(["noSession", "transactionalOnly", "maximumSessionTime"])(
  "unmeasured tree %s is missing",
  async (flag) => {
    expectEmulationFailure(`tree ${flag}`);
    const config = new JourneyConfig({
      realm: "alpha",
      trees: { invented: { entryNodeId: SUCCESS, nodes: {}, [flag]: true } },
      nodes: {},
      scripts: {},
    });
    expect(
      await request(
        new JourneyEmulator(
          config,
          runner,
          new IdmEmulator(new IdmConfig({ managed: { objects: [] } }), runner),
        ),
      ),
    ).toMatchObject({ status: 501, emulation: { missing: [`tree ${flag}`] } });
  },
);
it("the existing AIC driveJourney consumes this engine without a parallel callback driver", async () => {
  const { emitWrapperJourney } = await import("../../src/aic/emit-journey.ts");
  const { driveJourney } = await import("../../src/aic/run.ts");
  const { validateCase } = await import("../../src/case/index.ts");
  const { journeyHttp } = await import("../../src/journey/index.ts");
  const source =
    "if(callbacks.isEmpty()){callbacksBuilder.nameCallback('Name');}else{nodeState.putShared('answer',callbacks.getNameCallbacks().get(0));action.goTo('done');}";
  const first = validateCase({
    name: "invented chain",
    script: source,
    outcomes: ["done"],
    given: {},
    expect: { outcome: null, callbacks: [{ type: "NameCallback", prompt: "Name" }] },
  });
  const last = validateCase({
    name: "invented chain",
    script: source,
    outcomes: ["done"],
    given: {},
    expect: { outcome: "done" },
  });
  const wrapper = emitWrapperJourney(last, source, { runId: "invented", realm: "alpha" });
  const config = new JourneyConfig({
    realm: "alpha",
    trees: {
      [wrapper.treeName]:
        wrapper.treeBody as unknown as import("../../src/journey/types.ts").TreeDocument,
    },
    nodes: { ScriptedDecisionNode: wrapper.nodeBodies },
    scripts: Object.fromEntries(wrapper.scripts.map((s) => [s.id, script(s.source)])),
  });
  const emu = new JourneyEmulator(
    config,
    runner,
    new IdmEmulator(new IdmConfig({ managed: { objects: [] } }), runner),
  );
  const provider = {
    describe: async () => ({ name: "local", baseUrl: "https://tenant.example.com" }),
    getToken: async () => "unused",
  };
  const io = {
    http: journeyHttp(emu),
    aic: async () => {
      throw new Error("no CLI calls for authentication");
    },
  };
  const passes = await driveJourney(
    io,
    { tenantName: "local", baseUrl: "https://tenant.example.com", token: "unused", provider },
    wrapper,
    [first, last],
    [[{ type: "NameCallback", value: "invented" }]],
    { next: () => "invented-transaction" },
  );
  expect(passes).toHaveLength(2);
  expect(passes[0]?.callbacks).toEqual([{ type: "NameCallback", prompt: "Name" }]);
  expect(passes[1]?.outcome).toBe("done");
  expect(passes[1]?.evidence?.unbucketedState).toContainEqual(
    expect.objectContaining({ key: "answer", after: "invented" }),
  );
});
it("non-ASCII signature corruption remains the measured 400, never an internal error", async () => {
  const emu = make("callbacksBuilder.nameCallback('Name');");
  const first = await request(emu);
  const body = first.body as {
    authId: string;
    callbacks: import("../../src/protocol.ts").JsonValue[];
  };
  expect(await request(emu, { ...body, authId: body.authId.slice(0, -1) + "é" })).toMatchObject({
    status: 400,
    body: { message: "Authentication Error: Invalid AuthId Signature" },
  });
});
it("non-finite AM host results fail closed instead of turning into protocol marker objects", async () => {
  expectEmulationFailure("AM host non-finite or negative-zero result");
  const emu = make(
    "try{httpClient.send('https://service.example.com').get().json();}catch(e){}nodeState.putShared('username','invented');action.goTo('true');",
    undefined,
    undefined,
    { http: () => ({ status: 200, body: { value: NaN } }) },
  );
  expect(await request(emu)).toMatchObject({
    status: 501,
    emulation: { missing: ["AM host non-finite or negative-zero result"] },
  });
});
it("malformed HTTP JSON is audited missing through the authenticate seam", async () => {
  expectEmulationFailure("authenticate invalid JSON body");
  const { journeyHttp } = await import("../../src/journey/index.ts");
  const result = await journeyHttp(make("action.goTo('false');"))({
    url: "https://tenant.example.com/am/json/realms/root/realms/alpha/authenticate?authIndexType=service&authIndexValue=invented",
    method: "POST",
    headerLines: [],
    body: "{broken",
  });
  expect(result.status).toBe(501);
  expect(JSON.parse(result.body)).toMatchObject({
    message: expect.stringContaining("authenticate invalid JSON body"),
  });
});
it("AM harness timeout is sticky even when finally returns and the script chooses success", async () => {
  expectEmulationFailure("AM harness timeout");
  const emu = make(
    "(function(){try{while(true){}}finally{return 42;}})();nodeState.putShared('username','invented');action.goTo('true');",
    undefined,
    undefined,
    { timeoutMs: 80 },
  );
  expect(await request(emu)).toMatchObject({
    status: 500,
    emulation: { kind: "timeout", message: "AM harness timeout" },
  });
});
it("script cannot catch a missing host failure and perform a managed write", async () => {
  expectEmulationFailure("unmocked httpClient.send: https://service.example.com/check");
  const config = new JourneyConfig({
    realm: "alpha",
    trees: {
      invented: {
        entryNodeId: "one",
        nodes: { one: { nodeType: "ScriptedDecisionNode", connections: { true: SUCCESS } } },
      },
    },
    nodes: { ScriptedDecisionNode: { one: { script: "script", outcomes: ["true"] } } },
    scripts: {
      script: script(
        "try{httpClient.send('https://service.example.com/check').get();}catch(e){openidm.create('managed/widget','caught',{});}nodeState.putShared('username','invented');action.goTo('true');",
      ),
    },
  });
  const idm = new IdmEmulator(
    new IdmConfig({ managed: { objects: [{ name: "widget", schema: { properties: {} } }] } }),
    runner,
  );
  expect(await request(new JourneyEmulator(config, runner, idm))).toMatchObject({ status: 501 });
  expect((await idm.request({ method: "GET", path: "managed/widget/caught" })).status).toBe(404);
});
it("HTTP mock crashes are emulation failures even if script catches", async () => {
  expectEmulationFailure("journey HTTP mock failed: Error: broken mock");
  const emu = make(
    "try{httpClient.send('https://service.example.com').get();}catch(e){}nodeState.putShared('username','invented');action.goTo('true');",
    undefined,
    undefined,
    {
      http: () => {
        throw new Error("broken mock");
      },
    },
  );
  expect(await request(emu)).toMatchObject({ status: 500, emulation: { kind: "internal" } });
});
it("transition admission refuses an accidental loop with an audited protocol failure", async () => {
  expectEmulationFailure("journey transition limit exceeded");
  const emu = graph(
    {
      invented: {
        entryNodeId: "loop",
        nodes: { loop: { nodeType: "ScriptedDecisionNode", connections: { next: "loop" } } },
      },
    },
    { loop: "action.goTo('next');" },
    {},
    { maxTransitions: 2 },
  );
  expect(await request(emu)).toMatchObject({
    status: 500,
    emulation: { kind: "protocol", message: "journey transition limit exceeded" },
  });
});
it("unsupported legacy evaluator fails before execution", async () => {
  expectEmulationFailure("script evaluatorVersion: 1.0");
  const config = new JourneyConfig({
    realm: "alpha",
    trees: {
      invented: {
        entryNodeId: "one",
        nodes: { one: { nodeType: "ScriptedDecisionNode", connections: {} } },
      },
    },
    nodes: { ScriptedDecisionNode: { one: { script: "script", outcomes: [] } } },
    scripts: { script: { ...script("throw new Error('must not run');"), evaluatorVersion: "1.0" } },
  });
  expect(
    await request(
      new JourneyEmulator(
        config,
        runner,
        new IdmEmulator(new IdmConfig({ managed: { objects: [] } }), runner),
      ),
    ),
  ).toMatchObject({ status: 501, emulation: { missing: ["script evaluatorVersion: 1.0"] } });
});
it("tenant IDM conflicts stay catchable while the journey retains their host evidence", async () => {
  const config = new JourneyConfig({
    realm: "alpha",
    trees: {
      invented: {
        entryNodeId: "one",
        nodes: { one: { nodeType: "ScriptedDecisionNode", connections: {} } },
      },
    },
    nodes: { ScriptedDecisionNode: { one: { script: "script", outcomes: [] } } },
    scripts: {
      script: script(
        "openidm.create('managed/widget','record',{});try{openidm.create('managed/widget','record',{});}catch(e){logger.info('caught {}',e.code);callbacksBuilder.hiddenValueCallback('error',JSON.stringify({code:e.code,type:typeof e.code,detail:typeof e.detail}));}",
      ),
    },
  });
  const idm = new IdmEmulator(
    new IdmConfig({ managed: { objects: [{ name: "widget", schema: { properties: {} } }] } }),
    runner,
  );
  const result = await request(new JourneyEmulator(config, runner, idm));
  expect(result.status).toBe(200);
  expect(dump(result.body)).toEqual({ code: "412", type: "string", detail: "object" });
  expect(result.provenance).toContainEqual({
    kind: "assumed",
    behavior:
      "AM openidm errors use a JavaException-named JS adapter with string code and JSON detail",
  });
});
it("output-only callbacks count toward IDToken numbering and the existing client answers the right input", async () => {
  const emu = make(
    "if(callbacks.isEmpty()){callbacksBuilder.textOutputCallback(0,'hello');callbacksBuilder.nameCallback('Name');action.goTo('false');}else{nodeState.putShared('username',callbacks.getNameCallbacks().get(0));action.goTo('true');}",
  );
  const first = await request(emu);
  expect(first.body).toMatchObject({
    callbacks: [
      { type: "TextOutputCallback" },
      { type: "NameCallback", input: [{ name: "IDToken2", value: "" }] },
    ],
  });
  expect(
    await request(
      emu,
      JSON.parse(
        fillCallbackInputs(first.body, [{ type: "NameCallback", value: "invented" }], "numbering"),
      ),
    ),
  ).toMatchObject({ status: 200, body: { tokenId: expect.any(String) } });
});
it("unknown callback types and altered snapshot callback metadata remain missing", async () => {
  expectEmulationFailure("callback wire type: InventedCallback");
  expectEmulationFailure("authenticate callback submission differs from snapshot");
  const emu = make(
    "if(callbacks.isEmpty()){callbacksBuilder.nameCallback('Name');}else{nodeState.putShared('username','invented');action.goTo('true');}",
  );
  expect(await request(emu, { callbacks: [{ type: "InventedCallback" }] })).toMatchObject({
    status: 501,
    emulation: { missing: ["callback wire type: InventedCallback"] },
  });
  const first = await request(emu);
  const body = JSON.parse(
    fillCallbackInputs(first.body, [{ type: "NameCallback", value: "invented" }], "tamper"),
  );
  body.callbacks[0].output[0].value = "altered prompt";
  expect(await request(emu, body)).toMatchObject({
    status: 501,
    emulation: { missing: ["authenticate callback submission differs from snapshot"] },
  });
});
function chain(sources: string[], options: JourneyOptions = {}) {
  const ids = sources.map((_, index) => `n${index}`);
  return new JourneyEmulator(
    new JourneyConfig({
      realm: "alpha",
      trees: {
        invented: {
          entryNodeId: "n0",
          nodes: Object.fromEntries(
            ids.map((id, index) => [
              id,
              {
                nodeType: "ScriptedDecisionNode",
                connections: { next: ids[index + 1] ?? SUCCESS, true: SUCCESS },
              },
            ]),
          ),
        },
      },
      nodes: {
        ScriptedDecisionNode: Object.fromEntries(
          ids.map((id) => [id, { script: id, outcomes: ["next", "true"] }]),
        ),
      },
      scripts: Object.fromEntries(ids.map((id, index) => [id, script(sources[index] ?? "")])),
    }),
    runner,
    new IdmEmulator(new IdmConfig({ managed: { objects: [] } }), runner),
    options,
  );
}
const SUCCEED = "nodeState.putShared('username','invented');action.goTo('true');";
it.each([
  [
    "replaced bridge closure (review repro)",
    "httpClient.send.__parent__.host=function(){return '{\"result\":null}';};",
    "action.putSessionProperty('probe','value');",
    "action.putSessionProperty",
  ],
  [
    "replaced global stub hook",
    "__rhinoLocalNotMocked=function(){return null;};",
    "emailService.send('a','b','c');",
    "emailService.send",
  ],
  [
    "rebound Error.prototype.toString around a caught diagnostic",
    "Error.prototype.toString=function(){return 'safe';};",
    "try{nodeState.putShared('x',undefined);}catch(e){}",
    "nodeState.putShared: value is undefined",
  ],
  [
    "rebound arity helper around a caught diagnostic",
    "__rhinoLocalExpectArity=function(){};",
    "try{logger.info();}catch(e){}",
    "logger.info arity=0 (expected >= 1)",
  ],
])("author code cannot answer a refused call: %s", async (_name, attack, call, cause) => {
  expectEmulationFailure(cause);
  const result = await request(make(attack + call + SUCCEED));
  expect(result).toMatchObject({ status: 501, emulation: { missing: [cause] } });
});
it.each([
  ["cyclic object", "var o={};o.self=o;nodeState.putShared('x',o);", "cyclic object at x.self"],
  ["function", "nodeState.putShared('x',{f:function(){return 42;}});", "function at x.f"],
  ["NaN", "nodeState.putShared('x',NaN);", "non-finite or negative-zero number at x"],
  ["nested undefined", "nodeState.putShared('x',{u:undefined});", "undefined at x.u"],
  [
    "transient Infinity",
    "nodeState.putTransient('x',[1,Infinity]);",
    "non-finite or negative-zero number at x[1]",
  ],
])("unmeasured state shape is missing, not rewritten or a 401: %s", async (_name, store, shape) => {
  const bucket = store.includes("putTransient") ? "transient" : "shared";
  const cause = `journey state ${bucket} value: ${shape}`;
  expectEmulationFailure(cause);
  const result = await request(
    chain([
      store + "action.goTo('next');",
      "callbacksBuilder.hiddenValueCallback('state',JSON.stringify({value:nodeState.get('x')}));",
    ]),
  );
  expect(result).toMatchObject({ status: 501, emulation: { missing: [cause] } });
});
it("a cyclic value on the way to Success is missing, not a 401", async () => {
  const cause = "journey state shared value: cyclic object at x.self";
  expectEmulationFailure(cause);
  expect(
    await request(make("var o={};o.self=o;nodeState.putShared('x',o);" + SUCCEED)),
  ).toMatchObject({ status: 501, emulation: { missing: [cause] } });
});
it("state read back through nodeState wrappers stays representable", async () => {
  const result = await request(
    chain([
      "nodeState.mergeShared({objectAttributes:{x:'v'}});var a=nodeState.get('objectAttributes');a.put('y','w');nodeState.putShared('objectAttributes',a);action.goTo('next');",
      "callbacksBuilder.hiddenValueCallback('state',JSON.stringify(nodeState.get('objectAttributes')));",
    ]),
  );
  expect(result.status).toBe(200);
});
it("assumptions survive callback round trips into the final result", async () => {
  const emu = new JourneyEmulator(
    new JourneyConfig({
      realm: "alpha",
      trees: {
        x: {
          entryNodeId: "risk",
          nodes: {
            risk: { nodeType: "ExternalRiskNode", connections: { allow: "user" } },
            user: { nodeType: "UsernameCollectorNode", connections: { outcome: SUCCESS } },
          },
        },
      },
      nodes: { ExternalRiskNode: { risk: {} }, UsernameCollectorNode: { user: {} } },
      scripts: {},
    }),
    runner,
    new IdmEmulator(new IdmConfig({ managed: { objects: [] } }), runner),
    { declaredOutcomes: { risk: "allow" } },
  );
  const call = (body: JsonValue) =>
    emu.request({
      method: "POST",
      path: "authenticate",
      query: { authIndexType: "service", authIndexValue: "x" },
      body,
    });
  const declared = { kind: "assumed", behavior: "declared by test: x/risk -> allow" };
  const first = await call({});
  expect(first.provenance).toContainEqual(declared);
  const done = await call(
    JSON.parse(
      fillCallbackInputs(first.body, [{ type: "NameCallback", value: "invented" }], "provenance"),
    ),
  );
  expect(done).toMatchObject({ status: 200, body: { tokenId: expect.any(String) } });
  expect(done.provenance).toContainEqual(declared);
});
