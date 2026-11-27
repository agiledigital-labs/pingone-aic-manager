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
  // Review round 1 P1-1: the script's own variables and functions named like a binding are its own.
  it.each([
    ["a var", "var logger = 'local'; nodeState.putShared('username', logger); outcome = 'ok';"],
    ["an assignment", "httpClient = 'local'; nodeState.putShared('username', httpClient); outcome = 'ok';"],
    ["a later var", "var a = 1, openidm = 'local'; nodeState.putShared('username', openidm); outcome = 'ok';"],
    [
      "an uninitialised var",
      "var logger; nodeState.putShared('username', typeof logger === 'undefined' ? 'local' : 'tenant'); outcome = 'ok';",
    ],
    [
      "a function",
      "function utils() { return 'local'; } nodeState.putShared('username', utils()); outcome = 'ok';",
    ],
  ])("leaves %s named like a binding to the script", async (_, body) => {
    const done = await req(custom(body));
    expect(dumped(done.body).username).toBe("local");
  });
  // Review round 2 P1-1: a helper's locals, and a redundant top-level var, leave the wrapper's own
  // bindings alone. Discriminating: each name is the wrapper's (properties) or the guard's (action).
  it("keeps a helper-local properties apart from the injected one", async () => {
    const done = await req(
      custom(
        'function f() { var properties = "helper"; return properties; } nodeState.putShared("username", properties.greeting + "/" + f()); outcome = "ok";',
      ),
    );
    expect(dumped(done.body).username).toBe("hi/helper");
  });
  it("keeps the injected properties through a bare top-level var", async () => {
    const done = await req(
      custom("var properties; nodeState.putShared('username', properties.greeting); outcome = 'ok';"),
    );
    expect(dumped(done.body).username).toBe("hi");
  });
  it.each([
    ["a helper-local", "function f() { var action = 1; return action; } f();"],
    ["a top-level", "var action = 1;"],
  ])("routes past %s var action", async (_, prefix) => {
    const done = await req(custom(`${prefix} nodeState.putShared('username', 'local'); outcome = 'ok';`));
    expect(done.status).toBe(200);
    expect(dumped(done.body).username).toBe("local");
  });
  // The wrapper runs the script inside a block, where Rhino defines a function declaration only
  // when it is reached, so a released name read above its declaration must not be the harness's own
  // binding. (The tenant hoists it; that is unmodelled either way, but this way it is not a leak.)
  it("a function name read above its declaration is not the harness binding", async () => {
    const done = await req(
      custom("nodeState.putShared('username', typeof utils); outcome = 'ok'; function utils() {}"),
    );
    expect(dumped(done.body).username).not.toBe("object");
  });
  // Review round 2 P2-1: declarations come from the parse, not the text. Discriminating: each of the
  // first two is a declaration a regex misses; each of the last three is text a regex mistakes for one.
  it.each([
    ["a later uninitialised declarator", "var n = 1, logger;"],
    ["a declarator after a comment", "var /* local */ logger;"],
  ])("releases %s", async (_, prefix) => {
    const done = await req(
      custom(
        `${prefix} nodeState.putShared('username', typeof logger === 'undefined' ? 'local' : 'tenant'); outcome = 'ok';`,
      ),
    );
    expect(dumped(done.body).username).toBe("local");
  });
  it.each([
    ["a string", "var s = 'function logger() {}';"],
    ["a comment", "// function logger() {}\n"],
    ["a helper's body", "function f() { function logger() {} return logger; }"],
  ])("still refuses a free logger declared only in %s", async (_, prefix) => {
    const name = "custom node binding: logger";
    expectEmulationFailure(name);
    refused(await req(custom(`${prefix} logger.info('x'); outcome = 'ok';`)), name);
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

/** seed (puts shared username) -> SetPersistentCookieNode -> Success, so the cookie is set at once. */
function cookieJourney(cookie: Doc) {
  return new JourneyEmulator(
    new JourneyConfig({
      realm: "alpha",
      trees: {
        t: {
          entryNodeId: "seed",
          nodes: {
            seed: { nodeType: "ScriptedDecisionNode", connections: { next: "set" } },
            set: { nodeType: "SetPersistentCookieNode", connections: { outcome: SUCCESS } },
          },
        },
      },
      nodes: {
        ScriptedDecisionNode: { seed: { script: "seed", outcomes: ["next"] } },
        SetPersistentCookieNode: { set: cookie },
      },
      scripts: { seed: script("nodeState.putShared('username','neo');action.goTo('next');") },
    } as never),
    runner,
    idm(),
    { origin: "https://tenant.example.com" },
  );
}
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
    const done = await req(cookieJourney({ hmacSigningKey: KEY }));
    expect(done.status).toBe(200);
    const assumed = (done.provenance ?? [])
      .filter((entry) => entry.kind === "assumed")
      .map((entry) => entry.behavior)
      .join("\n");
    expect(assumed).toContain('uses "session-jwt"');
    expect(assumed).toContain("uses 5 hours");
    // Review round 1 P1-3: doc 25 measured the tenant Domain on this cookie.
    expect(done.cookies?.[0]?.domain).toBe("tenant.example.com");
    expect(assumed).not.toContain("no Domain attribute");
    // Review round 2 P1-2: the spelling is the emulator's choice, and says so.
    expect(assumed).toContain("Domain is the tenant host exactly");
  });

});

describe("review round 1 P1-3: the persistent cookie's measured Domain", () => {
  it("reaches the Set-Cookie header", async () => {
    const { journeyHttp } = await import("../../src/journey/index.ts");
    const KEY = Buffer.from("invented-persistent-key-0123456789").toString("base64");
    const res = await journeyHttp(cookieJourney({ persistentCookieName: "c", hmacSigningKey: KEY }))({
      // Review round 2 P1-2: a host other than the configured origin's, which must win.
      url: "https://login.example.com:7443/am/json/realms/root/realms/alpha/authenticate?authIndexType=service&authIndexValue=t",
      method: "POST",
      headerLines: [["content-type", "application/json"]],
      body: "{}",
    });
    const cookie = res.headers.find(([n, v]) => n === "set-cookie" && v.startsWith("c="))?.[1];
    expect(cookie).toContain("; Domain=login.example.com;");
  });
});

describe("P3-8 an unknown script logger level is refused, not dropped", () => {
  // Review round 1 P2-5: with no log sink the emitter never sees the line; the engine must refuse it.
  it("the engine refuses it, naming the level, with no log sink", async () => {
    // The real runner, with one line at a level the runtime never produces appended to its reply.
    const shim = Object.create(runner) as RhinoRunner;
    shim.eval = (async (...args: Parameters<RhinoRunner["eval"]>) => {
      const response = await runner.eval(...args);
      return {
        ...response,
        logs: [...(response.logs ?? []), { surface: "logger", level: "fatal", message: "x", args: [] }],
      };
    }) as RhinoRunner["eval"];
    const e = new JourneyEmulator(
      new JourneyConfig({
        realm: "alpha",
        trees: {
          t: {
            entryNodeId: "s",
            nodes: { s: { nodeType: "ScriptedDecisionNode", connections: { true: SUCCESS } } },
          },
        },
        nodes: { ScriptedDecisionNode: { s: { script: "s", outcomes: ["true"] } } },
        scripts: { s: script("logger.info('kept'); action.goTo('true');") },
      } as never),
      shim,
      idm(),
    );
    const name = "script logger level: fatal";
    expectEmulationFailure(name);
    refused(await req(e), name);
  });
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

describe("review round 1 P3-1: only the array auditEntryDetail is measured", () => {
  const journey = (assign: string) =>
    new JourneyEmulator(
      new JourneyConfig({
        realm: "alpha",
        trees: {
          t: { entryNodeId: "s", nodes: { s: { nodeType: "ScriptedDecisionNode", connections: { true: SUCCESS } } } },
        },
        nodes: { ScriptedDecisionNode: { s: { script: "s", outcomes: ["true"] } } },
        scripts: { s: script(`${assign} action.goTo('true');`) },
      } as never),
      runner,
      idm(),
    );
  const kinds = (provenance: readonly { kind: string; behavior: string }[] | undefined) =>
    (provenance ?? []).filter((p) => p.behavior.includes("auditEntryDetail")).map((p) => p.kind);
  it("an array is the measured 401", async () => {
    const r = await req(journey("auditEntryDetail = ['a'];"));
    expect(r.status).toBe(401);
    expect(kinds(r.provenance)).toEqual(["measured"]);
  });
  it("a number is the same 401, labelled assumed", async () => {
    const r = await req(journey("auditEntryDetail = 7;"));
    expect(r.status).toBe(401);
    expect(kinds(r.provenance)).toEqual(["assumed"]);
  });
});

describe("review round 1 P3-3: an ESV-backed OTP length is refused as unmodelled", () => {
  const build = (length: unknown) => () =>
    new JourneyConfig({
      realm: "alpha",
      trees: {
        t: { entryNodeId: "g", nodes: { g: { nodeType: "OneTimePasswordGeneratorNode", connections: { outcome: SUCCESS } } } },
      },
      nodes: { OneTimePasswordGeneratorNode: { g: { length } } },
      scripts: {},
    } as never);
  it("names the gap rather than the tenant's number refusal", () => {
    expect(build({ $int: "&{esv.otp.length}" })).toThrow(/Not modelled by the emulator: .*ESV placeholder/);
  });
  it("still gives the measured refusal for a number below 6", () => {
    expect(build(5)).toThrow("400 Data validation failed for the attribute, One Time Password Length");
  });
});
