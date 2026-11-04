import { afterAll, beforeAll, expect, it } from "vitest";
import { HostCallError, RhinoRunner } from "../../src/runner.ts";
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
  [
    "hidden toJSON",
    "var o={a:1};Object.defineProperty(o,'toJSON',{value:function(){return 'rewritten';}});nodeState.putShared('x',o);",
    "non-enumerable property at x.toJSON",
  ],
  [
    "hidden NaN",
    "var o={a:1};Object.defineProperty(o,'bad',{value:NaN});nodeState.putShared('x',o);",
    "non-enumerable property at x.bad",
  ],
  [
    "accessor",
    "var o={};Object.defineProperty(o,'g',{get:function(){return 1;},enumerable:true});nodeState.putShared('x',o);",
    "accessor property at x.g",
  ],
  [
    "custom prototype",
    "function C(){this.a=1;}nodeState.putShared('x',new C());",
    "non-plain prototype at x",
  ],
  ["null prototype", "nodeState.putShared('x',Object.create(null));", "non-plain prototype at x"],
  ["array expando", "var a=[1];a.k=2;nodeState.putShared('x',a);", "array property at x.k"],
  ["array hole", "nodeState.putShared('x',[1,,3]);", "array hole at x"],
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
it.each([
  [
    "JsonValue constructor",
    "try{new org.forgerock.json.JsonValue({a:1});}catch(e){}",
    "new JsonValue(...) is not modelled; use JsonValue.json or JsonValue.object",
  ],
  ["logger arity", "try{logger.info();}catch(e){}", "logger.info arity=0 (expected >= 1)"],
  ["request map arity", "try{requestHeaders.get();}catch(e){}", "map.get arity=0 (expected 1)"],
  [
    "utils arity",
    "try{utils.crypto.randomUUID(1);}catch(e){}",
    "utils.crypto.randomUUID arity=1 (expected 0)",
  ],
])(
  "a caught rhino-local diagnostic is missing, not a script error: %s",
  async (_name, call, cause) => {
    expectEmulationFailure(cause);
    expect(await request(make(call + SUCCEED))).toMatchObject({
      status: 501,
      emulation: { missing: [cause] },
    });
  },
);
it("measured utils members run locally and say so", async () => {
  const result = await request(
    make(
      "nodeState.putShared('username',utils.base64.encode('hi')+utils.crypto.randomUUID().length);action.goTo('true');",
    ),
  );
  expect(result.status).toBe(200);
  for (const member of ["utils.base64.encode", "utils.crypto.randomUUID"])
    expect(result.provenance).toContainEqual({
      kind: "measured",
      behavior: `${member} runs locally as measured`,
      source: "docs/api/12-script-bindings-matrix.md (utils, utils.crypto.subtle)",
    });
});
it.each([
  [
    "callback NaN",
    "callbacksBuilder.stringAttributeInputCallback('n','p','v',true,{bad:NaN},false,[]);",
    "journey callbacks value: non-finite or negative-zero number at callbacks[0].policies.bad",
  ],
  [
    "callback undefined",
    "callbacksBuilder.stringAttributeInputCallback('n','p','v',true,{},false,[undefined]);",
    "journey callbacks value: undefined at callbacks[0].failedPolicies[0]",
  ],
  [
    "cyclic host argument",
    "var o={};o.self=o;try{openidm.create('managed/widget',null,o);}catch(e){}" + SUCCEED,
    "AM host openidm.create argument: cyclic object at [2].self",
  ],
  [
    "undefined nested in create content",
    "openidm.create('managed/widget',null,{x:'a',n:{u:undefined}});" + SUCCEED,
    "AM host openidm.create argument: undefined at [2].n.u",
  ],
  [
    "undefined in update content",
    "openidm.update('managed/widget/x',null,{x:'a',u:undefined});" + SUCCEED,
    "AM host openidm.update argument: undefined at [2].u",
  ],
  [
    "undefined HTTP header",
    "httpClient.send('https://service.example.com/x',{method:'GET',headers:{h:undefined}}).get();" +
      SUCCEED,
    "AM host httpClient.send argument: undefined at [1].headers.h",
  ],
])("a value JSON would rewrite is missing: %s", async (_name, source, cause) => {
  expectEmulationFailure(cause);
  const result = await request(
    make(
      source,
      ["true", "false"],
      { true: SUCCESS, false: FAILURE },
      { http: async () => ({ status: 200, body: {} }) },
    ),
  );
  expect(result).toMatchObject({ status: 501, emulation: { missing: [cause] } });
});
const BODY_SOURCE =
  "docs/api/12-script-bindings-matrix.md (httpClient.send body serialization; §4 string escaping, 2026-11-04)";
const BODY_TEXT = {
  kind: "measured",
  behavior:
    'httpClient body text: an object body sent as Content-Type: application/json renders JS numbers as Java doubles (1 is 1.0) and undefined as null, and escapes strings and keys as measured (in printable ASCII only " and \\; other BMP raw; \\n, \\t short; other controls, astral and lone high surrogates as upper-case \\u escapes); no body key is empty',
  source: BODY_SOURCE,
};
const JSON_HEADER = "headers:{'Content-Type':'application/json'}";
const bodyAssumptions = (provenance: unknown) =>
  (provenance as { kind: string; behavior: string }[])
    .filter(
      (entry) => entry.kind === "assumed" && entry.behavior.startsWith("httpClient body text"),
    )
    .map((entry) => entry.behavior);
async function send(options: string) {
  const seen: { body?: unknown; has: boolean; text: string }[] = [];
  const result = await request(
    make(
      `httpClient.send('https://service.example.com/x',${options}).get();` + SUCCEED,
      ["true", "false"],
      { true: SUCCESS, false: FAILURE },
      {
        http: async (_url, sent, wire) => {
          seen.push({ body: sent.body, has: Object.hasOwn(sent, "body"), text: wire.bodyText });
          return { status: 200, body: {} };
        },
      },
    ),
  );
  expect(result.status).toBe(200);
  expect(seen).toHaveLength(1);
  return {
    ...seen[0]!,
    provenance: result.provenance,
    assumed: bodyAssumptions(result.provenance),
  };
}
it("an HTTP body reaches the mock as parsed JSON and as AIC's text (docs/api/12)", async () => {
  const sent = await send(
    `{method:'POST',${JSON_HEADER},body:{u:undefined,n:1,big:1000000,neg:-5,nested:{u:undefined},arr:[1,undefined],j:java.lang.Integer.valueOf(1),s:'s',t:true}}`,
  );
  // Parsed JSON cannot carry 1.0; the text the tenant would send does.
  expect(sent.body).toEqual({
    u: null,
    n: 1,
    big: 1000000,
    neg: -5,
    nested: { u: null },
    arr: [1, null],
    j: 1,
    s: "s",
    t: true,
  });
  expect(sent.text).toBe(
    '{"u":null,"n":1.0,"big":1000000.0,"neg":-5.0,"nested":{"u":null},"arr":[1.0,null],"j":1,"s":"s","t":true}',
  );
  expect(sent.provenance).toContainEqual(BODY_TEXT);
  expect(sent.provenance).toContainEqual({
    kind: "measured",
    behavior: "httpClient.send body undefined is sent as null",
    source: "docs/api/12-script-bindings-matrix.md (httpClient.send body serialization)",
  });
  expect(sent.assumed).toEqual([]);
});
it.each([
  ["body: undefined", `{method:'POST',${JSON_HEADER},body:undefined}`, true, null, "null"],
  ["body: null", `{method:'POST',${JSON_HEADER},body:null}`, true, null, "null"],
  ["no body key", `{method:'POST',${JSON_HEADER}}`, false, undefined, ""],
  // With no body there is no body text for a header to change.
  ["no body key and no header", "{method:'GET'}", false, undefined, ""],
])("HTTP %s is measured (docs/api/12)", async (_name, options, has, body, text) => {
  const sent = await send(options);
  expect(sent).toMatchObject({ has, body, text });
  expect(sent.provenance).toContainEqual(BODY_TEXT);
  expect(sent.assumed).toEqual([]);
});
const NO_HEADER = "httpClient body text without a Content-Type: application/json header";
const FRACTION =
  "httpClient body text for a fractional number or an integer past 1e6, written as a Java double";
const STRING = "httpClient body text for a string body, sent as the string itself";
const TOP = "httpClient body text for a top-level non-object body";
const INTEGER =
  "httpClient body text for a number the script engine holds as a Java Integer (results of length, indexOf, JSON.parse), written as a Java double";
it.each([
  ["a fraction", "body:{f:1.5}", '{"f":1.5}', [FRACTION]],
  ["an integer past 1e6", "body:{n:1e7}", '{"n":1.0E7}', [FRACTION]],
  ["a string body", "body:'raw'", "raw", [STRING]],
  ["a top-level array", "body:[1]", "[1.0]", [TOP]],
  ["a string length", "body:{count:'abc'.length}", '{"count":3.0}', [INTEGER]],
  ["an indexOf result", "body:{at:'abc'.indexOf('c')}", '{"at":2.0}', [INTEGER]],
  ["a JSON.parse result", "body:JSON.parse('{\"n\":2}')", '{"n":2.0}', [INTEGER]],
])("HTTP body text for %s is labelled assumed", async (_name, body, text, reasons) => {
  const sent = await send(`{method:'POST',${JSON_HEADER},${body}}`);
  if (text !== undefined) expect(sent.text).toBe(text);
  expect(sent.assumed).toEqual(reasons);
  expect(sent.provenance).not.toContainEqual(BODY_TEXT);
});
// docs/api/12 §4 (2026-11-04): exact wire bytes for strings and keys. Script literals are
// written as \u escapes so that Rhino reads U+2028 and lone surrogates as data, not source.
const lit = (text: string) =>
  `'${[...Array(text.length).keys()].map((i) => `\\u${text.charCodeAt(i).toString(16).padStart(4, "0")}`).join("")}'`;
const PRINTABLE = String.fromCharCode(...Array.from({ length: 0x5f }, (_, i) => 0x20 + i));
it.each([
  // Byte-identical to JSON.stringify: only " and \\ escaped, no \\/, < > & ' literal.
  [
    "every printable ASCII character",
    PRINTABLE,
    `"${PRINTABLE.replace(/[\\"]/g, (c) => `\\${c}`)}"`,
  ],
  ["a URL", "https://a.example.com/x", '"https://a.example.com/x"'],
  [
    "non-ASCII, U+2028, U+FEFF and U+007F",
    "\u00e9\u00a0\u2028\ufeff\u007f",
    '"\u00e9\u00a0\u2028\ufeff\u007f"',
  ],
  ["newline and tab", "a\nb\tc", '"a\\nb\\tc"'],
  ["C0 controls", "\u0000\u0001\u001f", '"\\u0000\\u0001\\u001F"'],
  ["an astral character", "\ud83d\ude00", '"\\uD83D\\uDE00"'],
  ["a lone high surrogate", "a\ud800b", '"a\\uD800b"'],
])("HTTP body text for %s is exact and measured", async (_name, value, quoted) => {
  for (const [body, text] of [
    [`{s:${lit(value)}}`, `{"s":${quoted}}`],
    // Keys are escaped like values.
    [`{${lit(value)}:'x'}`, `{${quoted}:"x"}`],
  ]) {
    const sent = await send(`{method:'POST',${JSON_HEADER},body:${body}}`);
    expect(sent.text).toBe(text);
    expect(sent.assumed).toEqual([]);
    expect(sent.provenance).toContainEqual(BODY_TEXT);
  }
});
it.each([
  [
    "a carriage return",
    "a\rb",
    '"a\\rb"',
    "httpClient body text for \\r, \\b or \\f in a string, written as a short escape (docs/api/12 §4 measured only \\n and \\t)",
  ],
  [
    "a C1 control",
    "\u0085",
    '"\u0085"',
    "httpClient body text for a C1 control character (U+0080-U+009F), written raw like the measured U+007F and other non-ASCII",
  ],
  [
    "a lone low surrogate",
    "\udc00",
    '"\\uDC00"',
    "httpClient body text for a lone low surrogate, written as an upper-case escape like the measured lone high surrogate",
  ],
])(
  "HTTP body text for %s is written as AIC likely would, labelled assumed",
  async (_name, value, quoted, reason) => {
    const sent = await send(`{method:'POST',${JSON_HEADER},body:{s:${lit(value)}}}`);
    expect(sent.text).toBe(`{"s":${quoted}}`);
    expect(sent.assumed).toEqual([reason]);
    expect(sent.provenance).not.toContainEqual(BODY_TEXT);
  },
);
it.each([
  ["no headers", "{method:'POST',body:{n:1}}"],
  ["another Content-Type", "{method:'POST',headers:{'Content-Type':'text/plain'},body:{n:1}}"],
])("an HTTP object body with %s is labelled assumed", async (_name, options) => {
  const sent = await send(options);
  expect(sent.text).toBe('{"n":1.0}');
  expect(sent.assumed).toEqual([NO_HEADER]);
  expect(sent.provenance).not.toContainEqual(BODY_TEXT);
});
// docs/api/12 §5 (2026-11-04): non-JSON values in an object body, as AIC sends them.
const NONJSON_SOURCE =
  "docs/api/12-script-bindings-matrix.md (§5 non-JSON values in an object body, 2026-11-04)";
const nonJson = (behavior: string) => ({ kind: "measured", behavior, source: NONJSON_SOURCE });
it("NaN, ±Infinity, -0 and java.util.Date in an HTTP body go out as AIC sends them, measured", async () => {
  const sent = await send(
    `{method:'POST',${JSON_HEADER},body:{a:'x',n:NaN,p:1/0,m:-1/0,z:-0,d0:new java.util.Date(0),d:new java.util.Date(1234)}}`,
  );
  expect(sent.text).toBe(
    '{"a":"x","n":"NaN","p":"Infinity","m":"-Infinity","z":-0.0,"d0":0,"d":1234}',
  );
  expect(Object.is((sent.body as { z: number }).z, -0)).toBe(true);
  expect(sent.body).toEqual({
    a: "x",
    n: "NaN",
    p: "Infinity",
    m: "-Infinity",
    z: -0,
    d0: 0,
    d: 1234,
  });
  expect(sent.assumed).toEqual([]);
  expect(sent.provenance).toContainEqual(BODY_TEXT);
  for (const behavior of [
    'httpClient.send body NaN and ±Infinity are sent as the strings "NaN", "Infinity" and "-Infinity"',
    "httpClient.send body -0 is sent as -0.0",
    "httpClient.send body java.util.Date is sent as its epoch milliseconds",
  ])
    expect(sent.provenance).toContainEqual(nonJson(behavior));
});
it("a §5 conversion inside an array is labelled assumed", async () => {
  const sent = await send(`{method:'POST',${JSON_HEADER},body:{arr:[NaN]}}`);
  expect(sent.text).toBe('{"arr":["NaN"]}');
  expect(sent.assumed).toEqual([
    "httpClient body text for NaN, ±Infinity, -0 or a java.util.Date inside an array, written as measured under an object key (docs/api/12 §5)",
  ]);
});
it("a cycle in an HTTP body throws AIC's catchable InternalError and sends nothing", async () => {
  const seen: unknown[] = [];
  const result = await request(
    make(
      "var b={a:'x',n:{}};b.n.up=b;var m='none';" +
        `try{httpClient.send('https://service.example.com/x',{method:'POST',${JSON_HEADER},body:b}).get();}catch(e){m=String(e);}` +
        "if(m==='InternalError: Cannot produce JSON from [object Object]'){" +
        SUCCEED +
        "}else{action.goTo('false');}",
      ["true", "false"],
      { true: SUCCESS, false: FAILURE },
      { http: async (url) => (seen.push(url), { status: 200, body: {} }) },
    ),
  );
  expect(result.status).toBe(200);
  expect(seen).toEqual([]);
  expect(result.provenance).toContainEqual(
    nonJson("httpClient.send body with a cycle throws a catchable InternalError; nothing is sent"),
  );
});
const DUMP = {
  date: "httpClient.send body.v is a JS Date: AIC sends a ~1 KB dump of the Rhino NativeDate object without the time value, not the date; send date.toISOString() or date.getTime() (docs/api/12 §5)",
  regexp:
    "httpClient.send body.v is a RegExp: AIC sends a ~1 KB dump of the Rhino NativeRegExp object without its source or flags; send regexp.source as a string (docs/api/12 §5)",
  fn: "httpClient.send body.v is a function: AIC sends a ~1.7 KB dump of the Rhino function object, which exposes the script's name, its encoded source and the name of every binding and variable in the script; leave it out of the body (docs/api/12 §5)",
};
it.each([
  ["a JS Date", "new Date(0)", DUMP.date],
  ["a RegExp", "/a/g", DUMP.regexp],
  ["a function", "function(){return 1;}", DUMP.fn],
  [
    "a toJSON method (never called)",
    "{toJSON:function(){return 'x';}}",
    DUMP.fn.replace("body.v", "body.v.toJSON"),
  ],
  [
    "a cycle through an array",
    "(function(){var l=[];l.push({l:l});return l;})()",
    "AM host httpClient.send argument: cyclic object at [1].body.v[0].l",
  ],
])("%s in an HTTP body is missing even when the send is caught", async (_name, value, cause) => {
  expectEmulationFailure(cause);
  const result = await request(
    make(
      `try{httpClient.send('https://service.example.com/x',{method:'POST',${JSON_HEADER},body:{a:'x',v:${value}}}).get();}catch(e){}` +
        SUCCEED,
      ["true", "false"],
      { true: SUCCESS, false: FAILURE },
      { http: async () => ({ status: 200, body: {} }) },
    ),
  );
  expect(result).toMatchObject({ status: 501, emulation: { missing: [cause] } });
});
it("an HTTP form body is missing: its text is not the JSON body (docs/api/12: form not probed)", async () => {
  const cause = "httpClient.send form body text";
  expectEmulationFailure(cause);
  const result = await request(
    make(
      "httpClient.send('https://service.example.com/x',{method:'POST',form:{a:'b'}}).get();" +
        SUCCEED,
      ["true", "false"],
      { true: SUCCESS, false: FAILURE },
      { http: async () => ({ status: 200, body: {} }) },
    ),
  );
  expect(result).toMatchObject({ status: 501, emulation: { missing: [cause] } });
});
it("withErrorMessage alongside queued callbacks is missing", async () => {
  const cause = "action.withErrorMessage with queued callbacks";
  expectEmulationFailure(cause);
  expect(
    await request(
      make(
        "callbacksBuilder.nameCallback('Name');action.goTo('false').withErrorMessage('discarded');",
      ),
    ),
  ).toMatchObject({ status: 501, emulation: { missing: [cause] } });
});
it("a non-array ScriptedDecisionNode outcomes configuration is missing, not a substring match", async () => {
  const cause = "ScriptedDecisionNode outcomes configuration";
  expectEmulationFailure(cause);
  expect(await request(make(SUCCEED, "true,false" as unknown as string[]))).toMatchObject({
    status: 501,
    emulation: { missing: [cause] },
  });
});
it("an undefined openidm.create content field is sent as null, so a schema default does not land (docs/api/12)", async () => {
  // docs/api/12 (2026-11-04): omitted gets the default; null and undefined are both refused
  // NOT_NULL. The policy evaluator stands in for the tenant's NOT_NULL guard.
  const idm = new IdmEmulator(
    new IdmConfig({
      managed: {
        objects: [
          {
            name: "widget",
            schema: {
              properties: { x: { type: "string" }, d: { type: "string", default: "def" } },
            },
          },
        ],
      },
    }),
    runner,
    {
      policy: ({ object }) => {
        if (object.d === null)
          throw new HostCallError({
            class: "org.forgerock.json.resource.ForbiddenException",
            code: 403,
            message: "Policy validation failed",
            detail: {
              failedPolicyRequirements: [
                { property: "d", policyRequirements: [{ policyRequirement: "NOT_NULL" }] },
              ],
            },
          });
      },
    },
  );
  const emu = new JourneyEmulator(
    new JourneyConfig({
      realm: "alpha",
      trees: {
        invented: {
          entryNodeId: "one",
          nodes: {
            one: {
              nodeType: "ScriptedDecisionNode",
              connections: { true: SUCCESS, false: FAILURE },
            },
          },
        },
      },
      nodes: { ScriptedDecisionNode: { one: { script: "script", outcomes: ["true", "false"] } } },
      scripts: {
        script: script(
          "var seen=[];" +
            "[['omitted',{x:'a'}],['null',{x:'a',d:null}],['undefined',{x:'a',d:undefined}]].forEach(function(c){" +
            "try{openidm.create('managed/widget',c[0],c[1]);seen.push(c[0]+':created');}" +
            "catch(e){seen.push(c[0]+':'+e.code);}});" +
            "nodeState.putShared('username','invented');" +
            "action.goTo(seen.join(',')==='omitted:created,null:403,undefined:403'?'true':'false');",
        ),
      },
    }),
    runner,
    idm,
  );
  const result = await request(emu);
  expect(result.status).toBe(200);
  const read = (id: string) => idm.request({ method: "GET", path: `managed/widget/${id}` });
  expect(await read("omitted")).toMatchObject({ status: 200, body: { d: "def" } });
  expect((await read("null")).status).toBe(404);
  expect((await read("undefined")).status).toBe(404);
  expect(result.provenance).toContainEqual({
    kind: "measured",
    behavior: "openidm.create content field holding undefined is sent as null",
    source:
      "docs/api/12-script-bindings-matrix.md (openidm.create content with an undefined value, 2026-11-04)",
  });
});
it.each(["httpClient.send();", "httpClient.send(undefined,{method:'GET'});"])(
  "httpClient.send with no URL is a missing journey behavior: %s",
  async (source) => {
    const cause = "httpClient.send with no URL";
    expectEmulationFailure(cause);
    expect(
      await request(
        make(
          source + SUCCEED,
          ["true", "false"],
          { true: SUCCESS, false: FAILURE },
          { http: async () => ({ status: 200, body: {} }) },
        ),
      ),
    ).toMatchObject({
      status: 501,
      emulation: { message: `Missing journey behavior: ${cause}`, missing: [cause] },
    });
  },
);
