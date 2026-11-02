import { afterAll, beforeAll, expect, it } from "vitest";
import { SUCCESS_NODE_ID as SUCCESS } from "../../src/aic/constants.ts";
import { expectEmulationFailure } from "../../src/harness/emulation-guard.ts";
import { HostCallError, RhinoRunner } from "../../src/runner.ts";
import { IdmConfig, IdmEmulator } from "../../src/idm/index.ts";
import {
  JourneyConfig,
  JourneyEmulator,
  type JourneyConfigInput,
  type JourneyOptions,
} from "../../src/journey/index.ts";
import { fillCallbackInputs } from "../../src/aic/callbacks.ts";
import type { JsonValue } from "../../src/protocol.ts";
let runner: RhinoRunner;
beforeAll(async () => {
  runner = await RhinoRunner.spawn();
}, 60_000);
afterAll(async () => {
  await runner?.close();
});
const script = (source: string) => ({
  evaluatorVersion: "2.0",
  script: Buffer.from(source).toString("base64"),
});
function config(children: string[], source: string, outcomes = ["chosen"]): JourneyConfigInput {
  return {
    realm: "alpha",
    trees: {
      invented: {
        entryNodeId: "page",
        nodes: {
          page: {
            nodeType: "PageNode",
            connections: {
              outcome: "dump",
              chosen: children.at(-1) === "child" ? "dump" : SUCCESS,
            },
          },
          dump: { nodeType: "ScriptedDecisionNode", connections: {} },
        },
      },
    },
    nodes: {
      PageNode: {
        page: {
          nodes: children.map((id) => ({
            _id: id,
            nodeType:
              id === "user"
                ? "UsernameCollectorNode"
                : id === "pass"
                  ? "PasswordCollectorNode"
                  : "ScriptedDecisionNode",
          })),
        },
      },
      UsernameCollectorNode: { user: {} },
      PasswordCollectorNode: { pass: {} },
      ScriptedDecisionNode: {
        child: { script: "child", outcomes },
        dump: { script: "dump", outcomes: [] },
      },
    },
    scripts: {
      child: script(source),
      dump: script(
        "callbacksBuilder.hiddenValueCallback('state',JSON.stringify({username:nodeState.get('username'),password:nodeState.get('password'),seen:nodeState.get('seen'),passes:nodeState.get('passes'),page:nodeState.isDefined('pageNodeCallbacks')}));",
      ),
    },
  };
}
function make(
  input: JourneyConfigInput,
  options: JourneyOptions = {},
  idm = new IdmEmulator(new IdmConfig({ managed: { objects: [] } }), runner),
) {
  return new JourneyEmulator(new JourneyConfig(input), runner, idm, options);
}
function request(emu: JourneyEmulator, body: JsonValue = {}) {
  return emu.request({
    method: "POST",
    path: "authenticate",
    query: { authIndexType: "service", authIndexValue: "invented" },
    body,
  });
}
function answer(body: JsonValue, values: string[]): JsonValue {
  const copy = structuredClone(body) as { callbacks: { input?: { value: JsonValue }[] }[] };
  copy.callbacks.forEach((cb, index) => {
    if (cb.input?.[0]) cb.input[0].value = values[index] ?? "";
  });
  return copy as JsonValue;
}
function dump(body: JsonValue): unknown {
  return JSON.parse(
    String(
      (
        body as { callbacks: { output: { name: string; value: JsonValue }[] }[] }
      ).callbacks[0]?.output.find((field) => field.name === "value")?.value,
    ),
  );
}
const child =
  "if(callbacks.isEmpty()){callbacksBuilder.textOutputCallback(0,'notice');callbacksBuilder.nameCallback('Script name');action.goTo('chosen');}else{nodeState.putShared('seen',{username:nodeState.get('username'),password:nodeState.get('password'),own:String(callbacks.getNameCallbacks().get(0))});action.goTo('chosen');}";
it.each([false, true])(
  "Page order, callback isolation and sequential state: script last=%s",
  async (last) => {
    const emu = make(config(last ? ["user", "pass", "child"] : ["user", "child", "pass"], child));
    const first = await request(emu);
    expect(first.status).toBe(200);
    const callbacks = (first.body as { callbacks: { type: string; input?: { name: string }[] }[] })
      .callbacks;
    expect(callbacks.map((cb) => cb.type)).toEqual(
      last
        ? ["NameCallback", "PasswordCallback", "TextOutputCallback", "NameCallback"]
        : ["NameCallback", "TextOutputCallback", "NameCallback", "PasswordCallback"],
    );
    expect(callbacks.at(-1)?.input?.[0]?.name).toBe("IDToken4");
    const next = await request(
      emu,
      answer(
        first.body,
        last ? ["invented", "secret", "", "own"] : ["invented", "", "own", "secret"],
      ),
    );
    expect(next.status).toBe(200);
    expect(dump(next.body)).toEqual({
      username: "invented",
      password: "secret",
      passes: null,
      seen: { username: "invented", password: last ? "secret" : null, own: "own" },
      page: true,
    });
    const later = await request(emu, next.body);
    expect(dump(later.body)).toMatchObject({ username: "invented", password: null, page: true });
  },
);
it("Page rejects a non-last multi-outcome child at config construction", () => {
  expect(() => new JourneyConfig(config(["child", "pass"], child, ["a", "b"]))).toThrow(
    "Invalid config: Only the last node in a page can have more than one outcome",
  );
});
it("a declared outcome cannot bypass an emulated Page", () => {
  expect(() =>
    make(config(["user", "pass"], child), { declaredOutcomes: { page: "outcome" } }),
  ).toThrow("declared outcome for emulated node");
});
it.each([false, true])(
  "Page no-callback script is not run at discovery, runs once on submit, last goTo wins: last=%s",
  async (last) => {
    const input = config(
      last ? ["user", "pass", "child"] : ["user", "child", "pass"],
      "nodeState.putShared('passes',(nodeState.get('passes')||0)+1);nodeState.putShared('seen',callbacks.getNameCallbacks().size());action.goTo('chosen');",
    );
    const emu = make(input),
      first = await request(emu);
    expect(
      (first.body as { callbacks: { type: string }[] }).callbacks.map((cb) => cb.type),
    ).toEqual(["NameCallback", "PasswordCallback"]);
    const next = await request(emu, answer(first.body, ["invented", "secret"]));
    expect(next.status).toBe(200);
    // Doc 25: not shown on the first response; on submission it runs once.
    expect(dump(next.body)).toMatchObject({
      passes: 1,
      seen: 0,
      username: "invented",
      password: "secret",
    });
  },
);
const PAGE_SPECULATION =
  "PageNode discovery classifies a scripted child by an isolated run (copied state, forked IDM, no HTTP); only a child that queues callbacks is then executed";
function widgets() {
  return new IdmEmulator(
    new IdmConfig({
      managed: { objects: [{ name: "widget", schema: { properties: { x: { type: "string" } } } }] },
    }),
    runner,
  );
}
async function widgetCount(idm: IdmEmulator): Promise<number> {
  const listed = await idm.request({
    method: "GET",
    path: "managed/widget",
    query: { _queryFilter: "true" },
  });
  return (listed.body as { result: unknown[] }).result.length;
}
it("Page discovery does not commit a callback-free child's host write", async () => {
  const idm = widgets();
  const emu = make(
    config(
      ["user", "child"],
      "openidm.create('managed/widget',null,{x:'created'});action.goTo('chosen');",
    ),
    {},
    idm,
  );
  const first = await request(emu);
  expect(first.status).toBe(200);
  expect(await widgetCount(idm)).toBe(0);
  const next = await request(emu, answer(first.body, ["invented"]));
  expect(next.status).toBe(200);
  expect(await widgetCount(idm)).toBe(1);
});
it("Page discovery runs a callback-emitting child for real exactly once", async () => {
  const idm = widgets();
  const emu = make(
    config(
      ["user", "child"],
      "if(callbacks.isEmpty()){openidm.create('managed/widget',null,{x:'created'});callbacksBuilder.nameCallback('Own');}else{action.goTo('chosen');}",
    ),
    {},
    idm,
  );
  const first = await request(emu);
  expect((first.body as { callbacks: { type: string }[] }).callbacks.map((cb) => cb.type)).toEqual([
    "NameCallback",
    "NameCallback",
  ]);
  expect(await widgetCount(idm)).toBe(1);
  expect(first.provenance).toContainEqual({
    kind: "assumed",
    behavior: PAGE_SPECULATION,
  });
  const next = await request(emu, answer(first.body, ["invented", "own"]));
  expect(next.status).toBe(200);
  expect(await widgetCount(idm)).toBe(1);
});
it("Page discovery reads state without failing the initial POST", async () => {
  const emu = make(
    config(
      ["user", "child"],
      "nodeState.putShared('seen',{username:nodeState.get('username')});action.goTo('chosen');",
    ),
  );
  const first = await request(emu);
  expect(first.status).toBe(200);
  const next = await request(emu, answer(first.body, ["invented"]));
  expect(dump(next.body)).toMatchObject({ seen: { username: "invented" } });
});
it("Page discovery of a child that throws without username is missing, not a 401", async () => {
  const cause = "PageNode scripted child that throws before its callbacks are known";
  expectEmulationFailure(cause);
  const emu = make(
    config(
      ["user", "child"],
      "if(nodeState.get('username')===null){throw new Error('username required');}action.goTo('chosen');",
    ),
  );
  expect(await request(emu)).toMatchObject({ status: 501, emulation: { missing: [cause] } });
});
it("Page discovery refuses HTTP before a child's callbacks are known", async () => {
  const cause = "PageNode scripted child httpClient.send before its callbacks are known";
  expectEmulationFailure(cause);
  let calls = 0;
  const emu = make(
    config(
      ["user", "child"],
      "httpClient.send('https://service.example.com/x',{}).get();action.goTo('chosen');",
    ),
    {
      http: async () => {
        calls += 1;
        return { status: 200, body: {} };
      },
    },
  );
  expect(await request(emu)).toMatchObject({ status: 501, emulation: { missing: [cause] } });
  expect(calls).toBe(0);
});
it("Page re-sent after a scripted child ran on submission is missing", async () => {
  const cause = "PageNode re-sent after a scripted child ran on submission";
  expectEmulationFailure(cause);
  const emu = make(
    config(
      ["user", "child"],
      "nodeState.putShared('passes',(nodeState.get('passes')||0)+1);callbacksBuilder.nameCallback('Again');",
    ),
  );
  const first = await request(emu);
  expect(first.status).toBe(200);
  expect(await request(emu, answer(first.body, ["invented", "own"]))).toMatchObject({
    status: 501,
    emulation: { missing: [cause] },
  });
});
it.each(["chosen", "unwired", "undeclared"])("Page last scripted outcome %s", async (outcome) => {
  const input = config(
    ["user", "child"],
    `if(callbacks.isEmpty()){callbacksBuilder.nameCallback('Own');}else{action.goTo('${outcome}');}`,
    ["chosen", "unwired"],
  );
  const emu = make(input),
    first = await request(emu);
  const next = await request(emu, answer(first.body, ["invented", "own"]));
  if (outcome === "chosen") expect(next.status).toBe(200);
  else if (outcome === "unwired")
    expect(next).toMatchObject({ status: 500, rawBody: "", body: null });
  else expect(next).toMatchObject({ status: 401, body: { message: "Login failure" } });
});
const stateDump =
  "callbacksBuilder.hiddenValueCallback('state',JSON.stringify({username:nodeState.get('username'),id:nodeState.get('_id'),attrs:nodeState.get('objectAttributes'),password:nodeState.get('password'),branch:nodeState.get('branch')}));";
function flow(
  type: string,
  node: Record<string, unknown>,
  seed: string,
  branches: string[] = ["outcome"],
  source = stateDump,
): JourneyConfigInput {
  return {
    realm: "alpha",
    trees: {
      invented: {
        entryNodeId: "seed",
        identityResource: "managed/widget",
        nodes: {
          seed: { nodeType: "ScriptedDecisionNode", connections: { next: "node" } },
          node: {
            nodeType: type,
            connections: Object.fromEntries(branches.map((branch) => [branch, branch])),
          },
          ...Object.fromEntries(
            branches.map((branch) => [
              branch,
              { nodeType: "ScriptedDecisionNode", connections: {} },
            ]),
          ),
        },
      },
    },
    nodes: {
      [type]: { node },
      ScriptedDecisionNode: {
        seed: { script: "seed", outcomes: ["next"] },
        ...Object.fromEntries(branches.map((branch) => [branch, { script: branch, outcomes: [] }])),
      },
    },
    scripts: {
      seed: script(seed + ";action.goTo('next');"),
      ...Object.fromEntries(
        branches.map((branch) => [
          branch,
          script(`nodeState.putShared('branch',${JSON.stringify(branch)});` + source),
        ]),
      ),
    },
  };
}
function storage(required = false) {
  return new IdmEmulator(
    new IdmConfig({
      managed: {
        objects: [
          {
            name: "widget",
            schema: {
              ...(required ? { required: ["sn"] } : {}),
              properties: {
                userName: { type: "string", title: "Username" },
                mail: { type: "string", title: "Email Address" },
                givenName: { type: "string", title: "First Name" },
                sn: { type: "string", title: "Last Name" },
                password: { type: "string", title: "Password" },
              },
            },
            onCreate: {
              type: "text/javascript",
              source: "object.sn = object.sn ? object.sn + '-hook' : object.sn;",
            },
          },
        ],
      },
    }),
    runner,
    {
      policy: ({ object }) => {
        if (required && !object.sn)
          throw new HostCallError({
            class: "org.forgerock.json.resource.BadRequestException",
            code: 403,
            message: "Required sn",
            detail: { policyRequirements: [{ policyRequirement: "REQUIRED" }] },
          });
      },
    },
  );
}
it.each([false, true])(
  "Attribute + validated password shadow whole objectAttributes: password=%s",
  async (password) => {
    const input = flow(
      "PageNode",
      {
        nodes: [
          { _id: "user", nodeType: "UsernameCollectorNode" },
          { _id: "attrs", nodeType: "AttributeCollectorNode" },
          ...(password ? [{ _id: "pass", nodeType: "ValidatedPasswordNode" }] : []),
        ],
      },
      "nodeState.putShared('objectAttributes',{before:'seed'});",
    );
    const nodes = {
      ...input.nodes,
      UsernameCollectorNode: { user: {} },
      AttributeCollectorNode: {
        attrs: { attributesToCollect: ["givenName", "sn"], validateInputs: false, required: true },
      },
      ValidatedPasswordNode: { pass: { validateInput: false, passwordAttribute: "password" } },
    };
    const emu = make({ ...input, nodes }, {}, storage()),
      first = await request(emu);
    const wire = first.body as {
      callbacks: {
        type: string;
        input: { name: string; value: JsonValue }[];
        output: { name: string; value: JsonValue }[];
      }[];
    };
    expect(wire.callbacks[1]).toEqual({
      type: "StringAttributeInputCallback",
      output: [
        { name: "name", value: "givenName" },
        { name: "prompt", value: "First Name" },
        { name: "required", value: true },
        { name: "policies", value: {} },
        { name: "failedPolicies", value: [] },
        { name: "validateOnly", value: false },
        { name: "value", value: "" },
      ],
      input: [
        { name: "IDToken2", value: "" },
        { name: "IDToken2validateOnly", value: false },
      ],
    });
    const rejected = await request(emu, answer(first.body, ["invented", "Given", "", "secret"]));
    expect(rejected.status).toBe(200);
    const retry = rejected.body as typeof wire;
    expect(retry.callbacks).toHaveLength(password ? 4 : 3);
    expect(retry.callbacks[1]?.output.find((field) => field.name === "value")?.value).toBe("Given");
    expect(
      retry.callbacks[2]?.output.find((field) => field.name === "failedPolicies")?.value,
    ).toEqual([]);
    const next = await request(
      emu,
      answer(rejected.body, ["invented", "Given", "Family", "secret"]),
    );
    expect(dump(next.body)).toMatchObject({
      username: "invented",
      attrs: password
        ? { password: "secret" }
        : { before: "seed", givenName: "Given", sn: "Family" },
      password: password ? "secret" : null,
    });
    expect(dump((await request(emu, next.body)).body)).toMatchObject({
      username: "invented",
      attrs: { before: "seed", givenName: "Given", sn: "Family" },
      password: null,
    });
  },
);
it.each(["validateOnly", "policy", "password"])(
  "unmeasured collector validation fails closed: %s",
  async (variant) => {
    const type = variant === "password" ? "ValidatedPasswordNode" : "AttributeCollectorNode";
    const input = flow(
      type,
      variant === "password"
        ? { validateInput: true }
        : { attributesToCollect: ["sn"], validateInputs: variant !== "validateOnly" },
      "",
    );
    const emu = make(input, {}, storage());
    const cause =
      variant === "password"
        ? "ValidatedPasswordNode validateInput: true or unspecified"
        : variant === "policy"
          ? "AttributeCollectorNode policy evaluation; supply the IDM policy seam for managed writes"
          : "AttributeCollectorNode validateOnly";
    expectEmulationFailure(cause);
    let result = await request(emu);
    if (variant === "validateOnly") {
      const posted = answer(result.body, ["Family"]) as {
        callbacks: { input: { value: JsonValue }[] }[];
      };
      const flag = posted.callbacks[0]?.input[1];
      if (flag) flag.value = true;
      result = await request(emu, posted as JsonValue);
    }
    expect(result).toMatchObject({ status: 501, emulation: { missing: [cause] } });
  },
);
it.each([
  ["customPassword", "ValidatedPasswordNode passwordAttribute: customPassword"],
  [undefined, "ValidatedPasswordNode passwordAttribute: unspecified"],
])("ValidatedPasswordNode passwordAttribute %s is missing", async (attribute, cause) => {
  expectEmulationFailure(cause);
  const emu = make(
    flow(
      "ValidatedPasswordNode",
      { validateInput: false, ...(attribute ? { passwordAttribute: attribute } : {}) },
      "",
    ),
    {},
    storage(),
  );
  expect(await request(emu)).toMatchObject({ status: 501, emulation: { missing: [cause] } });
});
it.each(["shared", "transient", "username", "none", "wrongAttribute"])(
  "Identify user with %s source",
  async (variant) => {
    const idm = storage();
    await idm.request({
      method: "PUT",
      path: "managed/widget/record",
      body: { userName: "invented", mail: "invented@example.com", sn: "Family" },
    });
    const seed =
      variant === "shared"
        ? "nodeState.putShared('objectAttributes',{mail:'invented@example.com'});"
        : variant === "transient"
          ? "nodeState.putTransient('objectAttributes',{mail:'invented@example.com'});"
          : `nodeState.putShared('username','${variant === "username" ? "invented@example.com" : variant === "none" ? "absent@example.com" : "invented"}');`;
    const emu = make(
      flow(
        "IdentifyExistingUserNode",
        { identityAttribute: "mail", identifier: "userName" },
        seed,
        ["true", "false"],
      ),
      {},
      idm,
    );
    const found = ["shared", "transient", "username"].includes(variant);
    const next = await request(emu);
    expect(dump(next.body)).toMatchObject({
      branch: found ? "true" : "false",
      id: found ? "record" : null,
      username: found ? "invented" : variant === "none" ? "absent@example.com" : "invented",
    });
    if (found) {
      expect(dump((await request(emu, next.body)).body)).toMatchObject({
        id: "record",
        username: "invented",
        attrs:
          variant === "shared"
            ? { mail: "invented@example.com", userName: "invented" }
            : { userName: "invented" },
      });
    }
  },
);
it.each(["shared", "transient", "required"])(
  "Create uses IDM hooks, leaves state untouched: %s",
  async (variant) => {
    const idm = storage(true),
      bucket = variant === "transient" ? "Transient" : "Shared";
    const seed = `nodeState.put${bucket}('objectAttributes',${variant === "required" ? "{userName:'invented'}" : "{userName:'invented',sn:'Family'}"});`;
    const emu = make(
      flow("CreateObjectNode", { identityResource: "managed/widget" }, seed, [
        "CREATED",
        "FAILURE",
      ]),
      {},
      idm,
    );
    const next = await request(emu);
    expect(dump(next.body)).toMatchObject({
      branch: variant === "required" ? "FAILURE" : "CREATED",
      id: null,
      username: null,
    });
    const query = await idm.request({
      method: "GET",
      path: "managed/widget",
      query: { _queryFilter: "true" },
    });
    const records = (query.body as { result: { sn: string }[] }).result;
    expect(records).toHaveLength(variant === "required" ? 0 : 1);
    if (variant !== "required") expect(records[0]?.sn).toBe("Family-hook");
    if (variant === "transient")
      expect(dump((await request(emu, next.body)).body)).toMatchObject({ attrs: null });
  },
);
it.each(["id", "attribute", "username", "transient", "absent"])(
  "Patch source discriminates FAILURE / PATCHED / abort: %s",
  async (variant) => {
    const idm = storage();
    await idm.request({
      method: "PUT",
      path: "managed/widget/record",
      body: { userName: "invented", sn: "Family" },
    });
    const seed =
      variant === "id"
        ? "nodeState.putShared('_id','record');nodeState.putShared('objectAttributes',{sn:'Changed'});"
        : variant === "username"
          ? "nodeState.putShared('username','invented');nodeState.putShared('objectAttributes',{sn:'Changed'});"
          : `nodeState.put${variant === "transient" ? "Transient" : "Shared"}('objectAttributes',{userName:'${variant === "absent" ? "nobody" : "invented"}',sn:'Changed'});`;
    const next = await request(
      make(
        flow(
          "PatchObjectNode",
          {
            identityResource: "managed/widget",
            identityAttribute: "userName",
            patchAsObject: false,
            ignoredFields: [],
          },
          seed,
          ["PATCHED", "FAILURE"],
        ),
        {},
        idm,
      ),
    );
    if (variant === "absent")
      expect(next).toMatchObject({ status: 401, body: { message: "Login failure" } });
    else
      expect(dump(next.body)).toMatchObject({ branch: variant === "id" ? "FAILURE" : "PATCHED" });
    expect(
      (await idm.request({ method: "GET", path: "managed/widget/record" })).body,
    ).toMatchObject({ sn: ["id", "absent"].includes(variant) ? "Family-hook" : "Changed" });
  },
);
it.each(["IdentifyExistingUserNode", "PatchObjectNode"])(
  "conflicting identity sources are missing: %s",
  async (type) => {
    const cause = `${type} conflicting identity sources`;
    expectEmulationFailure(cause);
    const idm = storage();
    for (const name of ["one", "two"])
      await idm.request({
        method: "PUT",
        path: `managed/widget/${name}`,
        body: { userName: name },
      });
    expect(
      await request(
        make(
          flow(
            type,
            { identityAttribute: "userName", identifier: "userName", patchAsObject: false },
            "nodeState.putShared('username','one');nodeState.putShared('objectAttributes',{userName:'two'});",
            ["true", "false"],
          ),
          {},
          idm,
        ),
      ),
    ).toMatchObject({ status: 501, emulation: { missing: [cause] } });
  },
);
it.each(["CreateObjectNode", "PatchObjectNode"])(
  "split objectAttributes is missing: %s",
  async (type) => {
    const cause = `${type} split objectAttributes across shared and transient`;
    expectEmulationFailure(cause);
    expect(
      await request(
        make(
          flow(
            type,
            { patchAsObject: false },
            "nodeState.putShared('objectAttributes',{userName:'invented'});nodeState.putTransient('objectAttributes',{password:'secret'});",
          ),
          {},
          storage(),
        ),
      ),
    ).toMatchObject({ status: 501, emulation: { missing: [cause] } });
  },
);
it("Patch patchAsObject is missing", async () => {
  const cause = "PatchObjectNode patchAsObject: true or unspecified";
  expectEmulationFailure(cause);
  expect(
    await request(make(flow("PatchObjectNode", { patchAsObject: true }, ""), {}, storage())),
  ).toMatchObject({ status: 501, emulation: { missing: [cause] } });
});
it("AttributeCollector accepts optional empty input and uses the common tenant callback filler", async () => {
  const emu = make(
    flow(
      "AttributeCollectorNode",
      { attributesToCollect: ["sn"], validateInputs: false, required: false },
      "",
    ),
    {},
    storage(),
  );
  const first = await request(emu);
  expect(
    (
      first.body as { callbacks: { output: { name: string; value: JsonValue }[] }[] }
    ).callbacks[0]?.output.find((field) => field.name === "required")?.value,
  ).toBe(false);
  const next = await request(
    emu,
    JSON.parse(
      fillCallbackInputs(
        first.body,
        [{ type: "StringAttributeInputCallback", value: "" }],
        "invented",
      ),
    ) as JsonValue,
  );
  expect(dump(next.body)).toMatchObject({ attrs: { sn: "" } });
});
it("whole-value transient shadowing also applies to an ordinary caller key", async () => {
  const input = flow(
    "UsernameCollectorNode",
    {},
    "nodeState.putShared('ordinary',{a:1});nodeState.putTransient('ordinary',{b:2});",
    ["outcome"],
    "callbacksBuilder.hiddenValueCallback('state',JSON.stringify(nodeState.get('ordinary')));",
  );
  // Callback-free seed -> script lets us observe both maps before any suspension.
  const tree = input.trees.invented;
  if (!tree) throw new Error("fixture tree missing");
  const modified = {
    ...input,
    trees: {
      invented: {
        ...tree,
        nodes: { ...tree.nodes, node: { nodeType: "ScriptedDecisionNode", connections: {} } },
      },
    },
    nodes: {
      ...input.nodes,
      ScriptedDecisionNode: {
        ...input.nodes.ScriptedDecisionNode,
        node: { script: "observe", outcomes: [] },
      },
    },
    scripts: {
      ...input.scripts,
      observe: script(
        "callbacksBuilder.hiddenValueCallback('state',JSON.stringify(nodeState.get('ordinary')));",
      ),
    },
  };
  const emu = make(modified),
    first = await request(emu);
  expect(dump(first.body)).toEqual({ b: 2 });
  expect(dump((await request(emu, first.body)).body)).toEqual({ a: 1 });
  expect(first.provenance).toContainEqual({
    kind: "assumed",
    behavior:
      "nodeState transient-first whole-value shadowing generalizes measured objectAttributes behavior to other keys",
  });
});
it("Identify followed by Success issues a session; Create alone does not invent a principal", async () => {
  const idm = storage();
  await idm.request({
    method: "PUT",
    path: "managed/widget/record",
    body: { userName: "invented", sn: "Family" },
  });
  for (const type of ["IdentifyExistingUserNode", "CreateObjectNode"]) {
    const input = flow(
      type,
      { identityAttribute: "userName", identifier: "userName", identityResource: "managed/widget" },
      "nodeState.putShared('objectAttributes',{userName:'invented',sn:'Family'});",
      type === "CreateObjectNode" ? ["CREATED"] : ["true"],
    );
    const tree = input.trees.invented;
    if (!tree) throw new Error("fixture tree missing");
    const changed = {
      ...input,
      trees: {
        invented: {
          ...tree,
          nodes: {
            ...tree.nodes,
            node: { nodeType: type, connections: { true: SUCCESS, CREATED: SUCCESS } },
          },
        },
      },
    };
    const result = await request(make(changed, {}, idm));
    if (type === "IdentifyExistingUserNode")
      expect(result).toMatchObject({ status: 200, body: { tokenId: expect.any(String) } });
    else expect(result).toMatchObject({ status: 500, body: null, rawBody: "" });
  }
});
