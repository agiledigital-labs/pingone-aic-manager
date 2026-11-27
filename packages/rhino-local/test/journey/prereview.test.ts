/**
 * Pre-review fixes (2026-11-25): each test drives the real JourneyEmulator through one input the
 * pre-review found handled silently or wrongly. Generic trees only.
 */
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { RhinoRunner } from "../../src/runner.ts";
import { IdmConfig, IdmEmulator } from "../../src/idm/index.ts";
import { JourneyConfig, JourneyEmulator, type JourneyOptions } from "../../src/journey/index.ts";
import { expectEmulationFailure } from "../../src/harness/emulation-guard.ts";
import type { JsonValue } from "../../src/protocol.ts";
import { SUCCESS_NODE_ID as SUCCESS } from "../../src/aic/constants.ts";

let runner: RhinoRunner;
beforeAll(async () => {
  runner = await RhinoRunner.spawn();
}, 60_000);
afterAll(async () => {
  await runner?.close();
});
type Doc = Record<string, unknown>;
const script = (source: string) => ({
  evaluatorVersion: "2.0",
  script: Buffer.from(source).toString("base64"),
});
const idm = () =>
  new IdmEmulator(
    new IdmConfig({
      sync: { mappings: [] },
      managed: {
        objects: [
          {
            name: "alpha_user",
            schema: {
              properties: {
                userName: { type: "string", title: "Username" },
                givenName: { type: "string", title: "First Name" },
              },
            },
          },
        ],
      },
    }),
    runner,
  );
/** The dump node reports shared state as JSON text through one HiddenValueCallback. */
const DUMP =
  "var o={};['objectAttributes','pageNodeCallbacks','username'].forEach(function(k){" +
  "var v=nodeState.get(k);o[k]=v===null?null:JSON.parse(JSON.stringify(v));});" +
  "callbacksBuilder.hiddenValueCallback('s',JSON.stringify(o));";
/** seed (script) -> node under test (or a PageNode `node`) -> dump. */
function emu(
  nodes: Record<string, Record<string, Doc>>,
  under: string,
  seed = "",
  outcomes: string[] = ["outcome"],
  options: JourneyOptions = {},
) {
  return new JourneyEmulator(
    new JourneyConfig({
      realm: "alpha",
      trees: {
        t: {
          entryNodeId: "seed",
          identityResource: "managed/alpha_user",
          nodes: {
            seed: { nodeType: "ScriptedDecisionNode", connections: { next: "node" } },
            node: {
              nodeType: under,
              connections: Object.fromEntries(outcomes.map((o) => [o, "dump"])),
            },
            dump: { nodeType: "ScriptedDecisionNode", connections: { true: SUCCESS } },
          },
        },
      },
      nodes: {
        ...nodes,
        ScriptedDecisionNode: {
          seed: { script: "seed", outcomes: ["next"] },
          dump: { script: "dump", outcomes: ["true"] },
        },
      },
      scripts: { seed: script(`${seed};action.goTo('next');`), dump: script(DUMP) },
    } as never),
    runner,
    idm(),
    options,
  );
}
const req = (e: JourneyEmulator, body: JsonValue = {}) =>
  e.request({
    method: "POST",
    path: "authenticate",
    query: { authIndexType: "service", authIndexValue: "t" },
    body,
  });
type Wire = {
  callbacks: {
    _id?: number;
    type: string;
    input: { name: string; value: JsonValue }[];
    output: { name: string; value: JsonValue }[];
  }[];
};
const fill = (body: JsonValue, values: Record<number, JsonValue>): JsonValue => {
  const copy = structuredClone(body) as Wire;
  for (const [index, value] of Object.entries(values)) {
    const input = copy.callbacks[Number(index)]?.input[0];
    if (!input) throw new Error(`no input at ${index}`);
    input.value = value;
  }
  return copy as unknown as JsonValue;
};
const dumped = (body: JsonValue): Doc =>
  JSON.parse(
    String((body as Wire).callbacks[0]?.output.find((f) => f.name === "value")?.value),
  ) as Doc;
/** Declares the one missing behaviour a test expects, then checks the 501 names it. */
const refused = (r: { status: number; body: JsonValue }, name: string): void => {
  expect(r.status).toBe(501);
  expect(String((r.body as Doc).message)).toContain(name);
};

describe("P2-1 ValidatedUsernameNode merges into shared objectAttributes", () => {
  const VU = { validateInput: false, usernameAttribute: "userName" };
  it("keeps keys an earlier node wrote", async () => {
    const e = emu(
      { ValidatedUsernameNode: { node: VU } },
      "ValidatedUsernameNode",
      "nodeState.putShared('objectAttributes',{givenName:'G'})",
    );
    const first = await req(e);
    const done = await req(e, fill(first.body, { 0: "neo" }));
    expect(dumped(done.body).objectAttributes).toEqual({ givenName: "G", userName: "neo" });
  });
  it("keeps a sibling Attribute Collector's answer on the same page", async () => {
    const e = emu(
      {
        PageNode: {
          node: {
            nodes: [
              { _id: "ac", nodeType: "AttributeCollectorNode" },
              { _id: "vu", nodeType: "ValidatedUsernameNode" },
            ],
          },
        },
        AttributeCollectorNode: {
          ac: { attributesToCollect: ["givenName"], validateInputs: false, required: false },
        },
        ValidatedUsernameNode: { vu: VU },
      },
      "PageNode",
    );
    const first = await req(e);
    const done = await req(e, fill(first.body, { 0: "G", 1: "neo" }));
    expect(dumped(done.body).objectAttributes).toEqual({ givenName: "G", userName: "neo" });
  });
  it("refuses a non-object shared objectAttributes rather than replacing it", async () => {
    const e = emu(
      { ValidatedUsernameNode: { node: VU } },
      "ValidatedUsernameNode",
      "nodeState.putShared('objectAttributes','text')",
    );
    const name = "ValidatedUsernameNode over non-object shared objectAttributes";
    expectEmulationFailure(name);
    const first = await req(e);
    refused(await req(e, fill(first.body, { 0: "neo" })), name);
  });
});

describe("P2-2 callback _id and pageNodeCallbacks", () => {
  const PAGE = {
    PageNode: {
      node: {
        nodes: [
          { _id: "u", nodeType: "UsernameCollectorNode" },
          { _id: "p", nodeType: "PasswordCollectorNode" },
        ],
      },
    },
    UsernameCollectorNode: { u: {} },
    PasswordCollectorNode: { p: {} },
  };
  it("numbers callbacks across the journey and maps each _id to its child", async () => {
    const e = emu(PAGE, "PageNode");
    const first = await req(e);
    expect((first.body as Wire).callbacks.map((cb) => cb._id)).toEqual([0, 1]);
    // An empty username re-sends the page; its callbacks are numbered on.
    const again = await req(e, fill(first.body, { 0: "", 1: "pw" }));
    expect((again.body as Wire).callbacks.map((cb) => cb._id)).toEqual([2, 3]);
    const done = await req(e, fill(again.body, { 0: "neo", 1: "pw" }));
    // The dump's own callback continues the count.
    expect((done.body as Wire).callbacks.map((cb) => cb._id)).toEqual([4]);
    expect(dumped(done.body).pageNodeCallbacks).toEqual({ "0": 0, "1": 1, "2": 0, "3": 1 });
  });
  it("refuses a pageNodeCallbacks that is not a map", async () => {
    const name = "pageNodeCallbacks that is not a map";
    expectEmulationFailure(name);
    refused(await req(emu(PAGE, "PageNode", "nodeState.putShared('pageNodeCallbacks','x')")), name);
  });
});

describe("P2-3 custom nodes reach only the measured bindings", () => {
  const SN = "designer-0123456789abcdef0123456789abcdef";
  function custom(body: string, errorOutcome = true) {
    return new JourneyEmulator(
      new JourneyConfig({
        realm: "alpha",
        trees: {
          t: {
            entryNodeId: "c",
            nodes: {
              c: { nodeType: SN, connections: { ok: "dump", "Script Error": "dump" } },
              dump: { nodeType: "ScriptedDecisionNode", connections: { true: SUCCESS } },
            },
          },
        },
        nodes: {
          [SN]: { c: { greeting: "hi" } },
          ScriptedDecisionNode: { dump: { script: "dump", outcomes: ["true"] } },
        },
        customNodeTypes: {
          [SN]: {
            outcomes: ["ok"],
            errorOutcome,
            properties: { greeting: { type: "STRING" } },
            script: body,
          },
        },
        scripts: { dump: script(DUMP) },
      } as never),
      runner,
      idm(),
      {},
    );
  }
  it("still gives properties, nodeState.get / putShared and outcome", async () => {
    const done = await req(
      custom(
        "nodeState.putShared('username', properties.greeting + '/' + nodeState.get('realm')); outcome = 'ok';",
      ),
    );
    expect(dumped(done.body).username).toBe("hi//alpha");
  });
  // Discriminating: with an error outcome, a plain throw would quietly take Script Error.
  it.each([
    ["openidm", "openidm.read('managed/alpha_user/x'); outcome = 'ok';"],
    ["httpClient", "var t = typeof httpClient; outcome = 'ok';"],
    ["callbacksBuilder", "callbacksBuilder.textOutputCallback(0, 'x'); outcome = 'ok';"],
    ["action", "action.goTo('ok');"],
    ["nodeState.putTransient", "nodeState.putTransient('x', 'y'); outcome = 'ok';"],
  ])("refuses %s by name", async (binding, body) => {
    const name = `custom node binding: ${binding}`;
    expectEmulationFailure(name);
    refused(await req(custom(body)), name);
  });
});

describe("P3-1 a blank answer is refused, not read as index 0", () => {
  it("MessageNode", async () => {
    const e = emu({ MessageNode: { node: { message: { en: "Go?" } } } }, "MessageNode", "", [
      "true",
      "false",
    ]);
    const first = await req(e);
    const name = 'MessageNode answer: ""';
    expectEmulationFailure(name);
    refused(await req(e, fill(first.body, { 1: "" })), name);
  });
  it("ChoiceCollectorNode", async () => {
    const e = emu(
      { ChoiceCollectorNode: { node: { choices: ["a", "b"], prompt: "Pick" } } },
      "ChoiceCollectorNode",
      "",
      ["a", "b"],
    );
    const first = await req(e);
    const name = 'ChoiceCollectorNode answer: " "';
    expectEmulationFailure(name);
    refused(await req(e, fill(first.body, { 0: " " })), name);
  });
  it("a numeric string still answers (the discriminating case)", async () => {
    const e = emu(
      { ChoiceCollectorNode: { node: { choices: ["a", "b"], prompt: "Pick" } } },
      "ChoiceCollectorNode",
      "",
      ["a", "b"],
    );
    const first = await req(e);
    const done = await req(e, fill(first.body, { 0: "1" }));
    expect(done.status).toBe(200);
  });
});

describe("P3-2 a persistent cookie needs a user to name", () => {
  const KEY = Buffer.from("invented-persistent-key-0123456789").toString("base64");
  const COOKIE = { persistentCookieName: "inventedCookie", hmacSigningKey: KEY, maxLife: 3 };
  it("refuses to mint with no shared username", async () => {
    const name = "SetPersistentCookieNode with no shared username";
    expectEmulationFailure(name);
    refused(
      await req(emu({ SetPersistentCookieNode: { node: COOKIE } }, "SetPersistentCookieNode")),
      name,
    );
  });
  it("mints with one, tagging the unconfigured defaults as assumed", async () => {
    const e = emu(
      { SetPersistentCookieNode: { node: { hmacSigningKey: KEY } } },
      "SetPersistentCookieNode",
      "nodeState.putShared('username','neo')",
    );
    const done = await req(e);
    expect(done.status).toBe(200);
    const assumed = (done.provenance ?? [])
      .filter((entry) => entry.kind === "assumed")
      .map((entry) => entry.behavior)
      .join("\n");
    expect(assumed).toContain('uses "session-jwt"');
    expect(assumed).toContain("uses 5 hours");
    expect(assumed).toContain("no Domain attribute");
  });
});

describe("P3-8 an unknown script logger level is refused, not dropped", () => {
  it("JourneyLogs refuses it", async () => {
    const { JourneyLogs } = await import("../../src/journey/emit.ts");
    const records: unknown[] = [];
    const logs = new JourneyLogs({ sink: { emit: (r: unknown) => records.push(r) } as never });
    const log = logs.begin({ method: "POST", path: "authenticate" } as never, "alpha");
    log.scriptLines([{ level: "info", message: "kept" }], "id", "name");
    expect(records.length).toBe(1);
    expect(() => log.scriptLines([{ level: "fatal", message: "x" }], "id", "name")).toThrow(
      'script logger level not modelled: "fatal"',
    );
  });
});

describe("P3-4 one localized-text rule for Page headers and node messages", () => {
  const message = (body: JsonValue) =>
    (body as Wire).callbacks[0]?.output.find((f) => f.name === "message")?.value;
  it("MessageNode shows the en entry of several, as a Page header does", async () => {
    const e = emu({ MessageNode: { node: { message: { fr: "F", en: "E" } } } }, "MessageNode", "", [
      "true",
      "false",
    ]);
    expect(message((await req(e)).body)).toBe("E");
  });
  it("MessageNode refuses several locales with no en", async () => {
    const e = emu({ MessageNode: { node: { message: { fr: "F", de: "D" } } } }, "MessageNode", "", [
      "true",
      "false",
    ]);
    const name = "MessageNode message with several locales and no en";
    expectEmulationFailure(name);
    refused(await req(e), name);
  });
});
