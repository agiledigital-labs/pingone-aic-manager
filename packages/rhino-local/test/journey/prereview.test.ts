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
import { FAILURE_NODE_ID as FAILURE, SUCCESS_NODE_ID as SUCCESS } from "../../src/aic/constants.ts";

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
  /**
   * A journey whose custom node `c` goes to `dump` by `ok` and by Script Error (or to `errorTo`;
   * `errDump` dumps like `dump` under the hidden callback id `err`, so the edge taken shows).
   * With `inner`, `c` sits in an inner tree that the outer tree evaluates before `dump`.
   */
  function custom(
    body: string,
    errorOutcome = true,
    using: RhinoRunner = runner,
    {
      errorTo = "dump",
      inner = false,
      esv = {},
    }: { errorTo?: string; inner?: boolean; esv?: Record<string, string> } = {},
  ) {
    const node = (next: string) => ({ nodeType: SN, connections: { ok: next, "Script Error": errorTo === "dump" ? next : errorTo } });
    const dump = { nodeType: "ScriptedDecisionNode", connections: { true: SUCCESS } };
    return new JourneyEmulator(
      new JourneyConfig({
        realm: "alpha",
        trees: inner
          ? {
              t: {
                entryNodeId: "e",
                nodes: { e: { nodeType: "InnerTreeEvaluatorNode", connections: { true: "dump", false: FAILURE } }, dump },
              },
              in: { entryNodeId: "c", innerTreeOnly: true, nodes: { c: node(SUCCESS) } },
            }
          : { t: { entryNodeId: "c", nodes: { c: node("dump"), dump, errDump: dump } } },
        nodes: {
          [SN]: { c: { greeting: "hi" } },
          ScriptedDecisionNode: {
            dump: { script: "dump", outcomes: ["true"] },
            errDump: { script: "errDump", outcomes: ["true"] },
          },
          ...(inner ? { InnerTreeEvaluatorNode: { e: { tree: "in" } } } : {}),
        },
        customNodeTypes: {
          [SN]: {
            outcomes: ["ok"],
            errorOutcome,
            properties: { greeting: { type: "STRING" } },
            script: body,
          },
        },
        scripts: { dump: script(DUMP), errDump: script(DUMP.replace("hiddenValueCallback('s'", "hiddenValueCallback('err'")) },
      } as never),
      using,
      idm(),
      { esv },
    );
  }
  /** The hidden callback id the dump went out under: `s` from `dump`, `err` from `errDump`. */
  const dumpedBy = (body: JsonValue): JsonValue | undefined =>
    (body as Wire).callbacks[0]?.output.find((f) => f.name === "id")?.value;
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
  // The script runs top-level, so the emulator hoists a function declaration as any script does
  // (the tenant's hoisting in a custom node is unrecorded): read above it, the name is the script's
  // own function, never the harness's binding.
  it("a function name read above its declaration is the script's own function", async () => {
    const done = await req(
      custom("nodeState.putShared('username', typeof utils); outcome = 'ok'; function utils() {}"),
    );
    expect(dumped(done.body).username).toBe("function");
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
  // Review round 4 (deviation 1): a bare typeof binds the name rather than reading it, and the
  // adapter cannot tell typeof from a declaration, so an unbound blocked name reads "undefined" as
  // any unbound name does. Its pair, typeof this.httpClient, is the named refusal (table above).
  it("a bare typeof of a blocked name the script never bound is \"undefined\"", async () => {
    const done = await req(custom("nodeState.putShared('username', typeof httpClient); outcome = 'ok';"));
    expect(dumped(done.body).username).toBe("undefined");
  });
  // Review round 6 P1: systemEnv.getProperty is measured in a custom node
  // (scripts/experiment-journey-esv-placeholders.sh).
  it("reads a configured ESV through systemEnv.getProperty and takes the listed edge", async () => {
    const done = await req(
      custom(
        "nodeState.putShared('username', systemEnv.getProperty('esv.round6.probe')); outcome = 'ok';",
        true,
        runner,
        { errorTo: "errDump", esv: { "esv.round6.probe": "from-esv" } },
      ),
    );
    expect(done.status).toBe(200);
    expect(dumpedBy(done.body)).toBe("s");
    expect(dumped(done.body).username).toBe("from-esv");
  });
  // Review round 7 P2-1: the view forwards get(key) and putShared(key, value) exactly, as before
  // ef89676d. Discriminating: Array.map passes (value, index, array), which the binding's own arity
  // check refuses, so forwarding every argument would take Script Error with username unset.
  it("forwards only the key when map passes nodeState.get extra arguments", async () => {
    const done = await req(
      custom(
        "nodeState.putShared('username', ['realm'].map(nodeState.get).join(','), 'extra'); outcome = 'ok';",
        true,
        runner,
        { errorTo: "errDump" },
      ),
    );
    expect(done.status).toBe(200);
    expect(dumpedBy(done.body)).toBe("s");
    expect(dumped(done.body).username).toBe("/alpha");
  });
  // Review round 8 P2-1: a plain read does not depend on the script's Array.prototype. Discriminating:
  // a view that built its argument list with push took Script Error with username unset.
  it("reads nodeState while the script has replaced Array.prototype.push", async () => {
    const done = await req(
      custom(
        "var push = Array.prototype.push, realm; Array.prototype.push = undefined; try { realm = nodeState.get('realm'); } finally { Array.prototype.push = push; } nodeState.putShared('username', realm); outcome = 'ok';",
        true,
        runner,
        { errorTo: "errDump" },
      ),
    );
    expect(done.status).toBe(200);
    expect(dumpedBy(done.body)).toBe("s");
    expect(dumped(done.body).username).toBe("/alpha");
  });
  // Review round 8 P3: getProperty forwards every argument, so its overloads and arity refusal are
  // the local Scripted Decision binding's: overloads assumed parity with doc 12, the arity basis
  // marked assumed in runtime.cjs; only getProperty(name) is measured.
  it.each([
    ["three arguments select the typed overload", "systemEnv.getProperty('esv.round8.n', '0', 'integer') + 1", "s", "8"],
    ["four arguments meet the binding's arity check", "systemEnv.getProperty('esv.round8.n', '0', 'integer', 'x')", "err", null],
  ])("getProperty: %s", async (_name, expr, edge, username) => {
    const done = await req(
      custom(`nodeState.putShared('username', String(${expr})); outcome = 'ok';`, true, runner, {
        errorTo: "errDump",
        esv: { "esv.round8.n": "7" },
      }),
    );
    expect(dumpedBy(done.body)).toBe(edge);
    expect(dumped(done.body).username).toBe(username);
  });
  // Review round 6 P1: the emulated systemEnv has no member but getProperty, so there is no other
  // member to refuse by name; an unknown one reads undefined and calling it throws a TypeError,
  // which routes like any throw. What the tenant's other members do is unmeasured.
  it("an unknown systemEnv member reads undefined, and calling it routes like a throw", async () => {
    const done = await req(
      custom(
        "nodeState.putShared('username', typeof systemEnv.getSecret); systemEnv.getSecret('esv.x'); outcome = 'ok';",
        true,
        runner,
        { errorTo: "errDump" },
      ),
    );
    expect(dumpedBy(done.body)).toBe("err");
    expect(dumped(done.body).username).toBe("undefined");
  });
  // Review round 6 P2-2: a throw ignores an outcome assigned before it. Discriminating: routing the
  // throw by the current outcome would take `ok` to `dump`.
  it("a throw after outcome = 'ok' takes Script Error and keeps the write", async () => {
    const done = await req(
      custom("outcome = 'ok'; nodeState.putShared('username', 'wrote'); throw 'boom';", true, runner, {
        errorTo: "errDump",
      }),
    );
    expect(done.status).toBe(200);
    expect(dumpedBy(done.body)).toBe("err");
    expect(dumped(done.body).username).toBe("wrote");
  });
  // Review round 6 P2-1: the no-route 401 for an unlisted outcome is assumed, and only there.
  it("labels the no-route unlisted outcome assumed, never measured", async () => {
    const done = await req(custom("outcome = 'wrong';", false));
    expect(done.status).toBe(401);
    // Review round 8 P2-2: the fixture's complete measured list, exactly, not a selection by prose
    // or citation, so the claim fails as a measured entry in any wording under any source.
    const kind = (k: string) => (done.provenance ?? []).filter((e) => e.kind === k);
    const SOURCE = "docs/api/25-journey-runtime.md (measurement agent, 2026-11-02)";
    expect(kind("measured")).toEqual([
      { kind: "measured", behavior: "unknown and innerTreeOnly direct journeys return No Configuration found", source: SOURCE },
      {
        kind: "measured",
        behavior:
          "a custom node runs its type's script with the instance's properties bound; a throw, an outcome the type does not list or an unset outcome takes Script Error when the type has an error outcome; without one a throw fails the journey with 401 Login failure; a missing required property fails it with 401; an outputs list does not restrict its writes",
        source: "docs/api/25-journey-runtime.md (custom node types, 2026-11-25)",
      },
      { kind: "measured", behavior: "next-gen nodeState has no getShared/getTransient; callback-free boundaries retain transient state", source: SOURCE },
      { kind: "measured", behavior: "action withErrorMessage works in either call order; withLockoutMessage has no visible failure-message effect", source: SOURCE },
    ]);
    expect(kind("assumed")).toContainEqual({
      kind: "assumed",
      behavior: "a custom node's unlisted or unset outcome without an error outcome fails the journey with 401 like a throw; unmeasured",
    });
  });
  // Review round 5 P2-3: outside the named refusals a custom script gets ordinary JavaScript errors,
  // which it can catch. Discriminating: a named refusal for these would be an uncatchable 501.
  it.each([
    ["nodeState.getShared", "nodeState.getShared('realm');", "TypeError"],
    ["nodeState.getTransient", "nodeState.getTransient('realm');", "TypeError"],
    ["an unknown global", "notABinding.read();", "ReferenceError"],
  ])("%s throws an ordinary catchable error", async (_, call, name) => {
    const done = await req(
      custom(`var n; try { ${call} n = 'none'; } catch (e) { n = e.name; } nodeState.putShared('username', n); outcome = 'ok';`),
    );
    expect(done.status).toBe(200);
    expect(dumped(done.body).username).toBe(name);
  });
  // Review round 4 P1-1: with no Script Error route a throw fails the journey and nothing it wrote
  // survives, so a write the harvest could not serialize must not become an emulation failure.
  it("a throw with no error outcome is 401 even after an unserializable write", async () => {
    const done = await req(custom("nodeState.putShared('bad', {u: undefined}); throw 'boom';", false));
    expect(done.status).toBe(401);
  });
  // Review round 4 P1-2: a computed string is a string; a String object is not one (Script Error,
  // here routed to failure so the two edges differ).
  it.each([
    ["a concatenation", "var suffix = 'k'; outcome = 'o' + suffix;", 200],
    ["a join", "outcome = ['o', 'k'].join('');", 200],
    ["a literal", "outcome = 'ok';", 200],
    ["a String object", "outcome = new String('ok');", 401],
  ])("routes %s by its value", async (_, body, status) => {
    const done = await req(custom(`nodeState.putShared('username', 'u'); ${body}`, true, runner, { errorTo: FAILURE }));
    expect(done.status).toBe(status);
  });
  // Review round 5 P1: the route is decided before anything is serialized, on every path. With no
  // route (no error outcome, and an unlisted or unset outcome) the journey fails and keeps nothing,
  // so a write the harvest could not serialize is never read: 401 at the top level, the named gap
  // in an inner tree. A throw was round 4's case.
  it.each([
    ["an unlisted outcome", "outcome = 'wrong';"],
    ["an unset outcome", ""],
    ["a throw", "throw 'boom';"],
  ])("%s with no route is 401 after an unserializable write", async (_, tail) => {
    const done = await req(custom(`nodeState.putShared('bad', {u: undefined}); ${tail}`, false));
    expect(done.status).toBe(401);
  });
  it.each([
    ["an unlisted outcome", "outcome = 'wrong';"],
    ["an unset outcome", ""],
  ])("%s with no route after an unserializable write stays the named gap in an inner tree", async (_, tail) => {
    const name = "inner tree script exception";
    expectEmulationFailure(name);
    refused(await req(custom(`nodeState.putShared('bad', {u: undefined}); ${tail}`, false, runner, { inner: true })), name);
  });
  // Review round 4 P2-2: in an inner tree, a throw or an unlisted outcome with no error outcome is
  // unmeasured, so it stays the named gap rather than the root's 401.
  it.each([
    ["a throw", "throw 'boom';"],
    ["an unlisted outcome", "outcome = 'wrong';"],
  ])("refuses %s in an inner tree with no error outcome by name", async (_, body) => {
    const name = "inner tree script exception";
    expectEmulationFailure(name);
    refused(await req(custom(body, false, runner, { inner: true })), name);
  });
  // Control for the inner-tree fixture: a listed outcome passes through it.
  it("an inner-tree custom node with a listed outcome finishes the outer tree", async () => {
    const done = await req(custom("nodeState.putShared('username', 'inner'); outcome = 'ok';", false, runner, { inner: true }));
    expect(done.status).toBe(200);
    expect(dumped(done.body).username).toBe("inner");
  });
  // Review round 3 P2-6: a type without a script is refused when the configuration loads, by name,
  // not as a TypeError from inside the engine on first use.
  it("refuses a custom type with no script at load", () => {
    const load = (def: Record<string, unknown>) => () =>
      new JourneyConfig({ realm: "alpha", trees: {}, nodes: {}, scripts: {}, customNodeTypes: { [SN]: def } } as never);
    expect(load({ outcomes: ["ok"], errorOutcome: true })).toThrow(
      `Invalid journey configuration: customNodeTypes/${SN}/script must be a string`,
    );
    // Control: the same type with a script loads.
    expect(load({ outcomes: ["ok"], errorOutcome: true, script: "outcome = 'ok';" })).not.toThrow();
  });
  // Review round 3: the script runs unmodified in its own top-level scope (java/CustomNodeScope.java).
  it.each([
    // P1-1: no wrapper temporary for a script to clobber.
    ["a var named like the old route temporary", "var __rhinoLocalCustomRoute = 'local'; nodeState.putShared('username', 'local'); outcome = 'ok';", "local"],
    // P1-2: top-level function declarations hoist.
    ["a function called above its declaration", "f(); outcome = 'ok'; function f() { nodeState.putShared('username', 'hoisted'); }", "hoisted"],
    // P2-1: a destructured declaration is the script's from the start.
    ["a destructured declaration read before it", "var before = typeof logger; var [logger] = ['local']; nodeState.putShared('username', before + '/' + logger); outcome = 'ok';", "undefined/local"],
    // P2-2: a custom script's auditEntryDetail is its own variable, not the Scripted Decision audit.
    ["a var auditEntryDetail", "var auditEntryDetail = 42; nodeState.putShared('username', 'local'); outcome = 'ok';", "local"],
    // Doc 12 (decision scripts): top-level const parses and reads back undefined.
    ["a top-level const", "const c = 'x'; nodeState.putShared('username', String(c)); outcome = 'ok';", "undefined"],
    // Doc 25: a throw takes Script Error and keeps what was written before it.
    ["a throw after a write", "nodeState.putShared('username', 'before'); throw 'x';", "before"],
  ])("runs %s as a top-level script", async (_, body, expected) => {
    const done = await req(custom(body));
    expect(done.status).toBe(200);
    expect(dumped(done.body).username).toBe(expected);
  });
  it("refuses a top-level let as a compilation failure, as doc 12 measured for decision scripts", async () => {
    const name = "journey script compilation failure";
    expectEmulationFailure(name);
    refused(await req(custom("let x = 1; outcome = 'ok';")), name);
  });
  describe("reports the author's own line numbers", () => {
    function capturing() {
      const seen: Awaited<ReturnType<RhinoRunner["eval"]>>[] = [];
      const shim = Object.create(runner) as RhinoRunner;
      shim.eval = (async (...args: Parameters<RhinoRunner["eval"]>) => {
        const response = await runner.eval(...args);
        seen.push(response);
        return response;
      }) as RhinoRunner["eval"];
      return { shim, seen };
    }
    it("for a runtime error", async () => {
      const { shim, seen } = capturing();
      const done = await req(custom("var a = 1;\nvar b = 2;\nnull.x;\noutcome = 'ok';", false, shim));
      expect(done.status).toBe(401);
      expect(seen[0]?.outcome).toBe("runtime_error");
      expect(seen[0]?.error?.line).toBe(3);
    });
    it("for a syntax error", async () => {
      const name = "journey script compilation failure";
      expectEmulationFailure(name);
      const { shim, seen } = capturing();
      refused(await req(custom("var a = 1;\nvar b = 2;\nvar = ;\noutcome = 'ok';", true, shim)), name);
      expect(seen[0]?.outcome).toBe("compile_error");
      expect(seen[0]?.error?.line).toBe(3);
    });
  });
  // Discriminating: with an error outcome, a plain throw would quietly take Script Error.
  it.each([
    ["openidm", "openidm.read('managed/alpha_user/x'); outcome = 'ok';"],
    // Review round 4: typeof through the global object is a property read, and refuses.
    ["httpClient", "var t = typeof this.httpClient; outcome = 'ok';"],
    ["callbacksBuilder", "callbacksBuilder.textOutputCallback(0, 'x'); outcome = 'ok';"],
    ["action", "action.goTo('ok');"],
    // Review round 3: a free read and a read through the global object both reach the refusal.
    ["logger", "logger.info('x'); outcome = 'ok';"],
    ["logger", "var l = this.logger; outcome = 'ok';"],
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
  // Review round 2 P3-1: only a real placeholder is the ESV form; a literal $int keeps its old path.
  it("keeps the number refusal for a literal {$int: \"7\"}", () => {
    expect(build({ $int: "7" })).toThrow("400 Data validation failed for the attribute, One Time Password Length");
    // Review round 3 P3-1: an empty name before the pipe is not a placeholder to the resolver either.
    expect(build({ $int: "&{esv.|7}" })).toThrow("400 Data validation failed for the attribute, One Time Password Length");
  });
});
