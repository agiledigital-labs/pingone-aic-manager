/** Journey emulator gaps, round 3: found by rewriting real journeys; generic trees only. */
import { createHmac } from "node:crypto";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { RhinoRunner } from "../../src/runner.ts";
import { IdmConfig, IdmEmulator } from "../../src/idm/index.ts";
import { JourneyConfig, JourneyEmulator, type JourneyOptions } from "../../src/journey/index.ts";
import type { JourneyConfigInput } from "../../src/journey/index.ts";
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

const b64 = (source: string) => Buffer.from(source).toString("base64");
const script = (source: string) => ({ evaluatorVersion: "2.0", script: b64(source) });
const ALICE = "00000000-0000-0000-0000-0000000000a1";
const BOB = "00000000-0000-0000-0000-0000000000b2";
async function users(): Promise<IdmEmulator> {
  const idm = new IdmEmulator(
    new IdmConfig({
      sync: { mappings: [] },
      managed: {
        objects: [
          {
            name: "alpha_user",
            schema: {
              properties: {
                userName: { type: "string", title: "Username" },
                mail: { type: "string", title: "Email Address" },
                password: { type: "string", title: "Password" },
                accountStatus: { type: "string", title: "Status" },
              },
            },
          },
        ],
      },
    }),
    runner,
  );
  for (const [id, name] of [
    [ALICE, "alice"],
    [BOB, "bob"],
  ] as const)
    await idm.request({
      method: "PUT",
      path: `managed/alpha_user/${id}`,
      body: { userName: name, mail: `${name}@example.com`, password: `${name}-secret`, accountStatus: "active" },
    });
  return idm;
}
function make(input: JourneyConfigInput, options: JourneyOptions = {}, idm?: IdmEmulator) {
  return new JourneyEmulator(
    new JourneyConfig(input),
    runner,
    idm ?? new IdmEmulator(new IdmConfig({ sync: { mappings: [] }, managed: { objects: [] } }), runner),
    options,
  );
}
const run = (emu: JourneyEmulator, tree: string, extra: Record<string, unknown> = {}) =>
  emu.request({
    method: "POST",
    path: "authenticate",
    query: { authIndexType: "service", authIndexValue: tree },
    body: {},
    ...extra,
  });
/** The JSON a state-dump script put in its first callback. */
function dump(body: JsonValue): Record<string, unknown> {
  const callbacks = (body as { callbacks?: { output: { name: string; value: JsonValue }[] }[] }).callbacks;
  return JSON.parse(String(callbacks?.[0]?.output.find((field) => field.name === "value")?.value)) as Record<
    string,
    unknown
  >;
}
const report = (branch: string) =>
  `callbacksBuilder.hiddenValueCallback('state',JSON.stringify({branch:'${branch}',username:nodeState.get('username')}));`;

// ---------------------------------------------------------------------------------------------
// Persistent cookies (batch 4 G23, G26, G30).
describe("persistent cookie", () => {
  const KEY = Buffer.from("invented-persistent-key-0123456789").toString("base64");
  const OTHER_KEY = Buffer.from("another-invented-key-0123456789ab").toString("base64");
  const COOKIE = { persistentCookieName: "inventedCookie", hmacSigningKey: KEY, maxLife: 3 };
  function journey(name: string, idm: IdmEmulator, options: JourneyOptions = {}, reader: Record<string, unknown> = {}) {
    return make(
      {
        realm: "alpha",
        trees: {
          setter: {
            entryNodeId: "seed",
            nodes: {
              seed: { nodeType: "ScriptedDecisionNode", connections: { next: "set" } },
              set: { nodeType: "SetPersistentCookieNode", connections: { outcome: SUCCESS } },
            },
          },
          reader: {
            entryNodeId: "read",
            identityResource: "managed/alpha_user",
            nodes: {
              read: { nodeType: "PersistentCookieDecisionNode", connections: { true: "yes", false: "no" } },
              yes: { nodeType: "ScriptedDecisionNode", connections: {} },
              no: { nodeType: "ScriptedDecisionNode", connections: {} },
            },
          },
        },
        nodes: {
          ScriptedDecisionNode: {
            seed: { script: "seed", outcomes: ["next"] },
            yes: { script: "yes", outcomes: [] },
            no: { script: "no", outcomes: [] },
          },
          SetPersistentCookieNode: { set: COOKIE },
          PersistentCookieDecisionNode: { read: { ...COOKIE, idleTimeout: 5, enforceClientIp: false, ...reader } },
        },
        scripts: {
          seed: script(`nodeState.putShared('username','${name}');action.goTo('next');`),
          yes: script(report("true")),
          no: script(report("false")),
        },
      },
      options,
      idm,
    );
  }
  const issued = async (emu: JourneyEmulator): Promise<string> => {
    const cookie = (await run(emu, "setter")).cookies?.[0];
    expect(cookie?.name).toBe("inventedCookie");
    return cookie?.value ?? "";
  };
  const read = (emu: JourneyEmulator, value: string) =>
    run(emu, "reader", { cookies: { inventedCookie: value } });
  const sign = (header: string, payload: string, key = KEY) =>
    `${header}.${payload}.${createHmac("sha256", Buffer.from(key, "base64")).update(`${header}.${payload}`).digest("base64url")}`;
  const part = (text: string) => Buffer.from(text).toString("base64url");

  it("G26: a request cookie needs no cookieName; a known session's value still does", async () => {
    // No cookieName configured: the persistent cookie reaches the node. A wrong implementation
    // that keeps refusing any cookie without cookieName fails here.
    const emu = journey("alice", await users());
    const value = await issued(emu);
    expect(dump((await read(emu, value)).body)).toMatchObject({ branch: "true", username: ALICE });
    // Discriminating: one that ignores cookies altogether would silently drop an existing session.
    expectEmulationFailure("session cookie name not supplied");
    const sessions = journey("alice", await users(), { sessions: { "session-value": { AuthLevel: "0" } } });
    const result = await run(sessions, "reader", { cookies: { someCookie: "session-value" } });
    expect(result.status).toBe(501);
  });

  it("G23: a decision node's signing key wrapped as $string reads like the literal", async () => {
    const emu = journey("alice", await users(), {}, { hmacSigningKey: { $string: KEY } });
    const value = await issued(emu);
    expect(dump((await read(emu, value)).body).branch).toBe("true");
    // Discriminating: the wrapper object itself used as a key could not verify its own cookie, and a
    // different wrapped key must not verify this one.
    const other = journey("alice", await users(), {}, { hmacSigningKey: { $string: OTHER_KEY } });
    expect(dump((await read(other, value)).body).branch).toBe("false");
  });

  it("G30: the value is an HS256 JWS over a five-part JWE, not a plain JWT", async () => {
    const value = await issued(journey("alice", await users()));
    const [head, payload, signature] = value.split(".") as [string, string, string];
    expect(JSON.parse(Buffer.from(head, "base64url").toString())).toEqual({ typ: "JWT", cty: "JWT", alg: "HS256" });
    expect(signature).toBe(
      createHmac("sha256", Buffer.from(KEY, "base64")).update(`${head}.${payload}`).digest("base64url"),
    );
    const inner = Buffer.from(payload, "base64url").toString();
    expect(inner.split(".")).toHaveLength(5);
    expect(JSON.parse(Buffer.from(inner.split(".")[0] as string, "base64url").toString())).toMatchObject({
      enc: "A128CBC-HS256",
    });
    // Discriminating: the claims are not readable in the outer payload, as they are in a plain JWT.
    expect(inner).not.toContain("alice");
    expect(value).not.toContain(Buffer.from("alice").toString("base64url"));
  });

  it("G30: a cookie signed by hand with the node's key is not accepted", async () => {
    const emu = journey("alice", await users());
    const head = part('{"typ":"JWT","cty":"JWT","alg":"HS256"}');
    // Plain claims under the right signing key, which is what a test author would write: the tenant
    // aborts with 401 (measured), and so does the emulator.
    const claims = part(JSON.stringify({ sub: "alice", iat: 1, exp: 4_102_444_800 }));
    expect(await read(emu, sign(head, claims))).toMatchObject({ status: 401 });
    // Five dotted parts that are not a JWE this emulator sealed: false, measured likewise.
    expect(dump((await read(emu, sign(head, part("not.a.jwe.at.all")))).body).branch).toBe("false");
  });

  it("G30: the identity is the encrypted part's: re-signing another user's inner token names that user", async () => {
    const idm = await users();
    const emu = journey("alice", idm);
    const bobs = journey("bob", idm);
    const alicesValue = await issued(emu);
    const bobsInner = ((await issued(bobs)).split(".") as string[])[1] as string;
    const head = (alicesValue.split(".") as string[])[0] as string;
    // Measured on the tenant: the outer signature covers the signing key only; the user comes from the
    // inner token. A wrong implementation keeping `sub` in the signed outer payload would name Alice.
    const swapped = await read(emu, sign(head, bobsInner));
    expect(dump(swapped.body)).toMatchObject({ branch: "true", username: BOB });
  });

  it("G30: damaged and short values take the measured branch", async () => {
    const emu = journey("alice", await users());
    const value = await issued(emu);
    const [head, payload] = value.split(".") as [string, string];
    const inner = Buffer.from(payload, "base64url").toString().split(".");
    inner[3] = `${(inner[3] as string).slice(0, -2)}AA`;
    expect(dump((await read(emu, sign(head, part(inner.join("."))))).body).branch).toBe("false");
    expect(dump((await read(emu, `${head}.${payload}`)).body).branch).toBe("false");
    expect(dump((await read(emu, sign(head, payload, OTHER_KEY))).body).branch).toBe("false");
    expect(await read(emu, "")).toMatchObject({ status: 401 });
    expect(await read(emu, "notajwt")).toMatchObject({ status: 401 });
    expectEmulationFailure("PersistentCookieDecisionNode over a cookie of 4 dotted parts");
    expect((await read(emu, `${value}.extra`)).status).toBe(501);
  });
});

// ---------------------------------------------------------------------------------------------
// ESV wrappers on a node-designer node's properties (batch 5 B5-6).
describe("ESV wrappers on custom node properties", () => {
  const DESIGNER = "designer-0123456789abcdef0123456789abcdef";
  const type = {
    outcomes: ["true", "false"],
    script: "outcome = String(properties.toggle === true && properties.limit === 7 && properties.label === 'a-x-b');",
    properties: {
      toggle: { type: "BOOLEAN", required: true },
      limit: { type: "NUMBER", required: false },
      label: { type: "STRING", required: false },
    },
  };
  const journey = (node: Record<string, unknown>, esv: Record<string, string | null> = {}) =>
    make(
      {
        realm: "alpha",
        trees: {
          invented: {
            entryNodeId: "n",
            nodes: {
              n: { nodeType: DESIGNER, connections: { true: "yes", false: "no" } },
              yes: { nodeType: "ScriptedDecisionNode", connections: {} },
              no: { nodeType: "ScriptedDecisionNode", connections: {} },
            },
          },
        },
        nodes: {
          [DESIGNER]: { n: node },
          ScriptedDecisionNode: { yes: { script: "yes", outcomes: [] }, no: { script: "no", outcomes: [] } },
        },
        scripts: { yes: script(report("true")), no: script(report("false")) },
        customNodeTypes: { [DESIGNER]: type },
      },
      { esv },
    );
  const ok = { limit: { $int: "7" }, label: "a-&{esv.invented.part}-b" };
  const esv = { "esv.invented.toggle": "true", "esv.invented.part": "x" };
  const branch = async (emu: JourneyEmulator) => dump((await run(emu, "invented")).body).branch;

  it("a $bool wrapper reads the configured ESV as a boolean", async () => {
    expect(await branch(journey({ ...ok, toggle: { $bool: "&{esv.invented.toggle}" } }, esv))).toBe("true");
    // Discriminating: "false" is a non-empty string, so a reader that leaves the wrapper's text as
    // the value, or coerces it by truthiness, would still take true here.
    const off = journey({ ...ok, toggle: { $bool: "&{esv.invented.toggle}" } }, { ...esv, "esv.invented.toggle": "false" });
    expect(await branch(off)).toBe("false");
  });

  it("$bool follows parseBoolean: a literal in any case is true, any other text is false", async () => {
    const take = async (text: string) => branch(journey({ ...ok, toggle: { $bool: text } }, esv));
    expect(await take("TRUE")).toBe("true");
    expect(await take("true")).toBe("true");
    expect(await take("maybe")).toBe("false");
    expect(await take("1")).toBe("false");
  });

  it("a $int wrapper on a NUMBER property reads a number; a non-integer is missing", async () => {
    const good = journey({ toggle: { $bool: "true" }, limit: { $int: "&{esv.invented.limit}" }, label: "a-x-b" }, { "esv.invented.limit": "7" });
    expect(await branch(good)).toBe("true");
    expectEmulationFailure("ESV placeholder resolved to a non-integer");
    const bad = await run(
      journey({ toggle: { $bool: "true" }, limit: { $int: "&{esv.invented.limit}" } }, { "esv.invented.limit": "1.5" }),
      "invented",
    );
    expect(bad.status).toBe(501);
  });

  it("a bare placeholder in a BOOLEAN property is missing: the tenant refuses it when the node is written", async () => {
    expectEmulationFailure("custom node property toggle typed BOOLEAN holding a string");
    const result = await run(journey({ ...ok, toggle: "&{esv.invented.toggle}" }, esv), "invented");
    expect(result.status).toBe(501);
  });

  it("an unconfigured placeholder is missing, not a default", async () => {
    expectEmulationFailure("ESV placeholder not configured: esv.invented.toggle");
    const result = await run(journey({ ...ok, toggle: { $bool: "&{esv.invented.toggle}" } }, { "esv.invented.part": "x" }), "invented");
    expect(result.status).toBe(501);
  });
});

// ---------------------------------------------------------------------------------------------
// Nodes as Page children: ValidatedUsernameNode, MessageNode, GetAuthenticatorAppNode (batch 3 G18,
// G21; batch 4 G25, G29). Doc 25, "Nodes as Page children".
describe("nodes as page children", () => {
  type Child = { type: string; config: Record<string, unknown> };
  const dumpOf = (branch: string) =>
    `callbacksBuilder.hiddenValueCallback('state',JSON.stringify({branch:'${branch}',username:nodeState.get('username'),` +
    `oa:nodeState.get('objectAttributes')?String(nodeState.get('objectAttributes')):null,` +
    `keys:nodeState.keys().toArray().sort()}));`;
  function pageInput(children: Child[], outcomes: string[]): JourneyConfigInput {
    const refs = children.map((child, index) => ({ _id: `c${index}`, nodeType: child.type }));
    const nodes: Record<string, Record<string, Record<string, unknown>>> = {
      PageNode: { page: { nodes: refs } },
      ScriptedDecisionNode: Object.fromEntries(outcomes.map((o) => [`d-${o}`, { script: `d-${o}`, outcomes: [] }])),
    };
    children.forEach((child, index) => {
      nodes[child.type] ??= {};
      (nodes[child.type] as Record<string, unknown>)[`c${index}`] = child.config;
    });
    return {
      realm: "alpha",
      trees: {
        invented: {
          entryNodeId: "page",
          nodes: {
            page: { nodeType: "PageNode", connections: Object.fromEntries(outcomes.map((o) => [o, `d-${o}`])) },
            ...Object.fromEntries(outcomes.map((o) => [`d-${o}`, { nodeType: "ScriptedDecisionNode", connections: {} }])),
          },
        },
      },
      nodes,
      scripts: Object.fromEntries(outcomes.map((o) => [`d-${o}`, script(dumpOf(o))])),
    };
  }
  type Wire = { callbacks: { type: string; output: { name: string; value: JsonValue }[]; input: { name: string; value: JsonValue }[] }[] };
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
  const out = (body: JsonValue, index: number, name: string) =>
    (body as Wire).callbacks[index]?.output.find((field) => field.name === name)?.value;
  const inputName = (body: JsonValue, index: number) => (body as Wire).callbacks[index]?.input[0]?.name;
  async function submit(children: Child[], outcomes: string[], values: Record<number, JsonValue>) {
    const emu = make(pageInput(children, outcomes));
    const first = await run(emu, "invented");
    const second = await run(emu, "invented", { body: answer(first.body, values) });
    return { first, second, state: second.status === 200 && "callbacks" in (second.body as object) ? dump(second.body) : undefined };
  }
  const NAME: Child = { type: "UsernameCollectorNode", config: {} };
  const APP: Child = {
    type: "GetAuthenticatorAppNode",
    config: {
      continueLabel: { en: "Continue" },
      message: { en: "Get the app {{appleLink}} {{googleLink}}" },
      appleLink: "https://apps.example.com/app/x",
      googleLink: "https://play.example.com/store/x",
    },
  };
  const MESSAGE: Child = {
    type: "MessageNode",
    config: { message: { en: "Hello" }, messageYes: { en: "Go" }, messageNo: { en: "Stop" } },
  };
  const VALIDATED: Child = {
    type: "ValidatedUsernameNode",
    config: { usernameAttribute: "userName", validateInput: false, autocompleteValues: [] },
  };

  it("GetAuthenticatorAppNode sends the script message and a continue option; any answer takes outcome", async () => {
    const emu = make(pageInput([APP], ["outcome"]));
    const first = await run(emu, "invented");
    expect(types(first.body)).toEqual(["TextOutputCallback", "ConfirmationCallback"]);
    expect(out(first.body, 0, "messageType")).toBe("4");
    expect(out(first.body, 0, "message")).toBe(
      `document.getElementById("callback_0").innerHTML="<center>Get the app <a target='_blank' href='https://apps.example.com/app/x'>Apple App Store</a> <a target='_blank' href='https://play.example.com/store/x'>Google Play Store</a></center>"`,
    );
    expect(out(first.body, 1, "options")).toEqual(["Continue"]);
    // Discriminating: an out-of-range index is still accepted (measured with 1 and 7), so a wrong
    // implementation validating the index like the Choice collector would fail here.
    for (const value of [0, 7]) {
      const next = await run(emu, "invented", { body: answer(first.body, { 1: value }) });
      expect(dump(next.body).branch).toBe("outcome");
    }
  });

  it("GetAuthenticatorAppNode with no configuration shows the default message and links", async () => {
    const first = await run(make(pageInput([{ type: "GetAuthenticatorAppNode", config: {} }], ["outcome"])), "invented");
    expect(out(first.body, 0, "message")).toBe(
      `document.getElementById("callback_0").innerHTML="<center>Get the app from the <a target='_blank' href='https://apps.apple.com/app/pingid/id891247102'>Apple App Store</a> or on <a target='_blank' href='https://play.google.com/store/apps/details?id=prod.com.pingidentity.pingid'>Google Play Store</a></center>"`,
    );
    expect(out(first.body, 1, "options")).toEqual(["Continue"]);
  });

  it("GetAuthenticatorAppNode with a wildcard it does not model is missing", async () => {
    expectEmulationFailure(
      "GetAuthenticatorAppNode message with a wildcard other than the two links, or a quote, backslash or line break",
    );
    const bad = { type: "GetAuthenticatorAppNode", config: { message: { en: "Get {{appleLabel}}" } } };
    expect((await run(make(pageInput([bad], ["outcome"])), "invented")).status).toBe(501);
  });

  it("a Page of a collector then the app node numbers the confirmation by position and leaves by outcome", async () => {
    const { first, state } = await submit([NAME, APP], ["outcome"], { 0: "someone", 2: 0 });
    expect(types(first.body)).toEqual(["NameCallback", "TextOutputCallback", "ConfirmationCallback"]);
    expect([0, 2].map((i) => inputName(first.body, i))).toEqual(["IDToken1", "IDToken3"]);
    expect(state).toMatchObject({ branch: "outcome", username: "someone" });
  });

  it("a Page whose only child is the app node or a Message node leaves no pageNodeCallbacks; with a collector it does", async () => {
    expect((await submit([APP], ["outcome"], { 1: 0 })).state?.keys).not.toContain("pageNodeCallbacks");
    expect((await submit([MESSAGE], ["true", "false"], { 1: 0 })).state?.keys).not.toContain("pageNodeCallbacks");
    expect((await submit([NAME, MESSAGE], ["true", "false"], { 0: "x", 2: 0 })).state?.keys).toContain("pageNodeCallbacks");
  });

  it("a MessageNode child sends its text and options; answer 0 takes true, any other index false", async () => {
    const { first, state } = await submit([NAME, MESSAGE], ["true", "false"], { 0: "someone", 2: 0 });
    expect(types(first.body)).toEqual(["NameCallback", "TextOutputCallback", "ConfirmationCallback"]);
    expect(out(first.body, 2, "options")).toEqual(["Go", "Stop"]);
    expect(state).toMatchObject({ branch: "true", username: "someone" });
    // Discriminating: both the "No" index and an out-of-range one take false (measured), not a refusal.
    for (const value of [1, 5])
      expect((await submit([NAME, MESSAGE], ["true", "false"], { 0: "someone", 2: value })).state?.branch).toBe("false");
  });

  it("a MessageNode that is not the last child is refused when the page is configured", () => {
    expect(() => new JourneyConfig(pageInput([MESSAGE, NAME], ["outcome"]))).toThrow(
      "Only the last node in a page can have more than one outcome",
    );
    // Control: the same two nodes the other way round are accepted.
    expect(() => new JourneyConfig(pageInput([NAME, MESSAGE], ["true", "false"]))).not.toThrow();
  });

  it("ValidatedUsernameNode as a child writes username and objectAttributes.userName; the later writer wins username", async () => {
    const after = await submit([NAME, VALIDATED], ["outcome"], { 0: "first", 1: "second" });
    expect(types(after.first.body)).toEqual(["NameCallback", "NameCallback"]);
    expect(after.state).toMatchObject({ branch: "outcome", username: "second" });
    expect(String(after.state?.oa)).toContain("second");
    // Discriminating: with the validated node first, the collector writes username last but never
    // objectAttributes, so the two keys then name different answers; an implementation in which
    // either node always wins would give the same answer in both orders.
    const before = await submit([VALIDATED, NAME], ["outcome"], { 0: "first", 1: "second" });
    expect(before.state).toMatchObject({ branch: "outcome", username: "second" });
    expect(String(before.state?.oa)).toContain("first");
  });

  it("an empty answer to a ValidatedUsernameNode child writes nothing and the page still leaves", async () => {
    const { state } = await submit([VALIDATED], ["outcome"], { 0: "" });
    expect(state).toMatchObject({ branch: "outcome", username: null, oa: null });
  });

  it("ValidatedUsernameNode with validateInput true as a child is missing", async () => {
    expectEmulationFailure("ValidatedUsernameNode validateInput: true as a PageNode child");
    const strict = { type: VALIDATED.type, config: { ...VALIDATED.config, validateInput: true } };
    expect((await run(make(pageInput([strict], ["outcome"])), "invented")).status).toBe(501);
  });
});

// ---------------------------------------------------------------------------------------------
// AttributeCollector boolean attributes (batch 3 G19, batch 4 G27). Doc 25, "Attribute Collector
// boolean attributes".
describe("attribute collector boolean attributes", () => {
  const widget = () =>
    new IdmEmulator(
      new IdmConfig({
        sync: { mappings: [] },
        managed: {
          objects: [
            {
              name: "widget",
              schema: {
                properties: {
                  givenName: { type: "string", title: "First Name" },
                  trusted: { type: "boolean", title: "Trust this device" },
                  preferences: {
                    type: "object",
                    properties: { remember: { type: "boolean", title: "Remember me" } },
                  },
                },
              },
            },
          ],
        },
      }),
      runner,
    );
  const tree = (attributes: string[], extra: Record<string, unknown> = {}): JourneyConfigInput => ({
    realm: "alpha",
    trees: {
      invented: {
        entryNodeId: "at",
        nodes: {
          at: { nodeType: "AttributeCollectorNode", connections: { outcome: "dump" } },
          dump: { nodeType: "ScriptedDecisionNode", connections: { true: SUCCESS } },
        },
      },
    },
    nodes: {
      AttributeCollectorNode: {
        at: { identityResource: "managed/widget", attributesToCollect: attributes, validateInputs: false, required: false, ...extra },
      },
      ScriptedDecisionNode: { dump: { script: "dump", outcomes: ["true"] } },
    },
    scripts: {
      dump: script(
        `var oa = nodeState.get('objectAttributes'); var t = oa.get('trusted');` +
          `callbacksBuilder.hiddenValueCallback('state', JSON.stringify({trusted: t === null ? null : String(t), typeofTrusted: typeof t, nested: JSON.stringify(oa.get('preferences')), given: String(oa.get('givenName'))}));`,
      ),
    },
  });
  type Wire = { callbacks: { type: string; output: { name: string; value: JsonValue }[]; input: { name: string; value: JsonValue }[] }[] };
  const answerWith = (body: JsonValue, values: JsonValue[]): JsonValue => {
    const copy = structuredClone(body) as Wire;
    copy.callbacks.forEach((cb, i) => {
      const input = cb.input[0];
      if (input) input.value = values[i] ?? input.value;
    });
    return copy as unknown as JsonValue;
  };

  it("sends a BooleanAttributeInputCallback defaulting to false, with a validateOnly input", async () => {
    const first = await run(make(tree(["trusted", "givenName"]), {}, widget()), "invented");
    const [boolean, string] = (first.body as Wire).callbacks;
    expect(boolean?.type).toBe("BooleanAttributeInputCallback");
    expect(Object.fromEntries((boolean?.output ?? []).map((f) => [f.name, f.value]))).toEqual({
      name: "trusted",
      prompt: "Trust this device",
      required: false,
      policies: {},
      failedPolicies: [],
      validateOnly: false,
      value: false,
    });
    expect(boolean?.input.map((i) => [i.name, i.value])).toEqual([
      ["IDToken1", false],
      ["IDToken1validateOnly", false],
    ]);
    // Control: the string attribute beside it stays a String callback numbered after it.
    expect(string?.type).toBe("StringAttributeInputCallback");
    expect(string?.input[0]?.name).toBe("IDToken2");
  });

  it("stores the boolean as a boolean, not the string \"true\"", async () => {
    const emu = make(tree(["trusted", "givenName"]), {}, widget());
    const first = await run(emu, "invented");
    const done = await run(emu, "invented", { body: answerWith(first.body, [true, "Zz"]) });
    // Discriminating: an implementation that types every attribute as string (the demo's workaround)
    // would store "true" and report typeof string.
    expect(dump(done.body)).toMatchObject({ trusted: "true", typeofTrusted: "boolean", given: "Zz" });
  });

  it("false is a value: a required boolean answered false leaves the node and is stored", async () => {
    const emu = make(tree(["trusted"], { required: true }), {}, widget());
    const first = await run(emu, "invented");
    const done = await run(emu, "invented", { body: answerWith(first.body, [false]) });
    expect(dump(done.body)).toMatchObject({ trusted: "false", typeofTrusted: "boolean" });
  });

  it("a nested boolean path stores nested", async () => {
    const emu = make(tree(["preferences/remember"]), {}, widget());
    const first = await run(emu, "invented");
    const done = await run(emu, "invented", { body: answerWith(first.body, [true]) });
    expect(dump(done.body).nested).toBe('{"remember":true}');
  });

  it("validateInputs true over a boolean needs no policy and sends empty policies", async () => {
    const emu = make(tree(["trusted"], { validateInputs: true }), { attributePolicies: {} }, widget());
    const first = await run(emu, "invented");
    expect(Object.fromEntries((first.body as Wire).callbacks[0]!.output.map((f) => [f.name, f.value])).policies).toEqual({});
    const done = await run(emu, "invented", { body: answerWith(first.body, [true]) });
    expect(dump(done.body).trusted).toBe("true");
  });

  it("answering a boolean attribute with a string is a 400, not a coercion", async () => {
    const emu = make(tree(["trusted"]), {}, widget());
    const first = await run(emu, "invented");
    const bad = structuredClone(first.body) as Wire;
    (bad.callbacks[0]!.input[0] as { value: JsonValue }).value = "true";
    const next = await run(emu, "invented", { body: bad as unknown as JsonValue });
    expect(next).toMatchObject({ status: 400, body: { code: 400, reason: "Bad Request" } });
  });
});

// ---------------------------------------------------------------------------------------------
// DataStoreDecisionNode and accountStatus (batch 3 G27, batch 4 G24). Doc 25, "DataStoreDecisionNode
// and accountStatus".
describe("DataStoreDecisionNode ignores accountStatus", () => {
  const tree = (): JourneyConfigInput => ({
    realm: "alpha",
    trees: {
      invented: {
        entryNodeId: "seed",
        nodes: {
          seed: { nodeType: "ScriptedDecisionNode", connections: { true: "ds" } },
          ds: { nodeType: "DataStoreDecisionNode", connections: { true: "t", false: "f" } },
          t: { nodeType: "ScriptedDecisionNode", connections: {} },
          f: { nodeType: "ScriptedDecisionNode", connections: {} },
        },
      },
    },
    nodes: {
      DataStoreDecisionNode: { ds: {} },
      ScriptedDecisionNode: {
        seed: { script: "seed", outcomes: ["true"] },
        t: { script: "t", outcomes: [] },
        f: { script: "f", outcomes: [] },
      },
    },
    scripts: {
      seed: script(`nodeState.putShared('username','PERSON');nodeState.putTransient('password','pw-PERSON');action.goTo('true');`),
      t: script(report("true")),
      f: script(report("false")),
    },
  });
  async function verdict(status: string | undefined, password = "pw-PERSON") {
    const idm = new IdmEmulator(
      new IdmConfig({
        sync: { mappings: [] },
        managed: { objects: [{ name: "alpha_user", schema: { properties: { userName: { type: "string", title: "u" }, password: { type: "string", title: "p" }, accountStatus: { type: "string", title: "s" } } } }] },
      }),
      runner,
    );
    await idm.request({
      method: "PUT",
      path: "managed/alpha_user/00000000-0000-0000-0000-0000000000c3",
      body: { userName: "PERSON", password: "pw-PERSON", ...(status === undefined ? {} : { accountStatus: status }) },
    });
    const base = tree();
    const input = {
      ...base,
      scripts: {
        ...base.scripts,
        seed: script(`nodeState.putShared('username','PERSON');nodeState.putTransient('password','${password}');action.goTo('true');`),
      },
    };
    const result = await run(make(input, {}, idm), "invented");
    return dump(result.body).branch;
  }
  // Discriminating: every status, including the ones a status check would refuse, gives the password
  // verdict (measured on the tenant: "inactive" and "Inactive" with the right password took true).
  it.each(["active", "Active", "inactive", "Inactive", "", undefined])("status %j with the right password takes true", async (status) => {
    expect(await verdict(status)).toBe("true");
  });
  it("a wrong password still takes false whatever the status", async () => {
    for (const status of ["active", "inactive", undefined])
      expect(await verdict(status, "wrong")).toBe("false");
  });
});

// ---------------------------------------------------------------------------------------------
// action.putSessionProperty in next-gen decision scripts (batch 3 G17, batch 5 B5-5). Doc 25,
// "Session properties from scripts".
describe("session properties from scripts", () => {
  const chain = (bodies: string[], callbackAfter = false): JourneyConfigInput => {
    const ids = bodies.map((_, i) => `s${i}`);
    if (callbackAfter) ids.push("wait");
    return {
      realm: "alpha",
      trees: {
        invented: {
          entryNodeId: ids[0] as string,
          identityResource: "managed/alpha_user",
          nodes: Object.fromEntries(
            ids.map((id, i) => [id, { nodeType: "ScriptedDecisionNode", connections: { true: ids[i + 1] ?? SUCCESS } }]),
          ),
        },
      },
      nodes: {
        ScriptedDecisionNode: Object.fromEntries(ids.map((id) => [id, { script: id, outcomes: ["true"] }])),
      },
      scripts: {
        ...Object.fromEntries(
          bodies.map((body, i) => [`s${i}`, script((i === 0 ? "nodeState.putShared('username','alice');" : "") + body + "action.goTo('true');")]),
        ),
        wait: script(`if (callbacks.isEmpty()) { callbacksBuilder.hiddenValueCallback('w','x'); } else { action.goTo('true'); }`),
      },
    };
  };
  const finish = async (input: JourneyConfigInput) => {
    const emu = make(input, {}, await users());
    let result = await run(emu, "invented");
    if ("callbacks" in (result.body as object)) result = await run(emu, "invented", { body: result.body });
    return result;
  };

  it("puts properties on the session of a successful journey, as strings, chained", async () => {
    const result = await finish(chain([`action.putSessionProperty('a', '1').putSessionProperty('b', 5);`]));
    expect(result.status).toBe(200);
    // Discriminating: a number value arrives as the string "5" (measured), not the number.
    expect(result.session?.properties).toEqual({ a: "1", b: "5" });
  });

  it("a later script overwrites an earlier script's key and can remove another of its keys", async () => {
    const result = await finish(
      chain([
        `action.putSessionProperty('a', 'first'); action.putSessionProperty('b', 'keep');`,
        `action.putSessionProperty('a', 'second'); action.removeSessionProperty('b');`,
      ]),
    );
    // Discriminating: if the second script could not see the first's keys, 'b' would survive.
    expect(result.session?.properties).toEqual({ a: "second" });
  });

  it("put then remove in one script leaves nothing, and no session object is invented", async () => {
    const result = await finish(chain([`action.putSessionProperty('a', '1'); action.removeSessionProperty('a');`]));
    expect(result.status).toBe(200);
    expect(result.session?.properties ?? {}).toEqual({});
  });

  it("keeps a property across a callback round trip", async () => {
    const result = await finish(chain([`action.putSessionProperty('a', 'kept');`], true));
    expect(result.session?.properties).toEqual({ a: "kept" });
  });

  it.each(["UserId", "AuthLevel", "Service", "Locale"])("putting %s, which AM owns, fails the journey with 401 Login failure", async (name) => {
    const result = await finish(chain([`action.putSessionProperty('${name}', 'x');`]));
    expect(result).toMatchObject({ status: 401, body: { message: "Login failure" } });
  });

  it("putting an AM-owned property not measured for scripts is missing", async () => {
    expectEmulationFailure("putSessionProperty of a property AM owns: Host");
    expect((await finish(chain([`action.putSessionProperty('Host', 'x');`]))).status).toBe(501);
  });
});

// ---------------------------------------------------------------------------------------------
// HttpMock connection failure (batch 3 G25, batch 5 B5-3). Doc 25, "httpClient connection failures".
describe("httpClient connection failures", () => {
  const probe = `
    var out = { where: 'none' };
    var pending = null;
    try { pending = httpClient.send('http://invented.example.com:8080/x', { method: 'GET' }); out.sent = typeof pending; }
    catch (e) { out.where = 'send'; }
    if (pending) {
      try { out.status = pending.get().status; }
      catch (e) { out.where = 'get'; out.name = e.name; out.message = String(e.message); out.string = String(e); out.isError = e instanceof Error; out.keys = Object.keys(e).join(','); }
    }
    callbacksBuilder.hiddenValueCallback('res', JSON.stringify(out));`;
  const tree = (): JourneyConfigInput => ({
    realm: "alpha",
    trees: {
      invented: {
        entryNodeId: "s",
        nodes: { s: { nodeType: "ScriptedDecisionNode", connections: { true: SUCCESS } } },
      },
    },
    nodes: { ScriptedDecisionNode: { s: { script: "s", outcomes: ["true"] } } },
    scripts: { s: script(probe + "action.goTo('true');") },
  });
  const PREFIX =
    "java.util.concurrent.ExecutionException: org.forgerock.openam.scripting.wrappers.HttpClientScriptException: java.lang.RuntimeException: ";
  async function seen(failure: unknown) {
    const emu = make(tree(), { http: () => ({ failure }) as never });
    return dump((await run(emu, "invented")).body);
  }
  it.each([
    [{ kind: "refused" }, "java.net.ConnectException: Connection refused (connect failed)"],
    [{ kind: "unknown-host" }, "java.net.UnknownHostException: invented.example.com: Name or service not known"],
    [
      { kind: "connect-timeout", address: "192.0.2.1", port: 81 },
      "java.net.ConnectException: Timeout connecting to [/192.0.2.1:81]",
    ],
  ])("%j is thrown by get(), not send(), as a JavaException", async (failure, tail) => {
    const state = await seen(failure);
    // Discriminating: where it throws. An implementation throwing from send() reports where: 'send'.
    expect(state).toMatchObject({
      where: "get",
      sent: "object",
      name: "JavaException",
      message: PREFIX + tail,
      string: `JavaException: ${PREFIX}${tail}`,
      isError: true,
    });
    // name is not an own enumerable key (measured keys: message,fileName,lineNumber,stack,...).
    expect(String(state.keys)).not.toContain("name");
  });
  it("an uncaught connection failure ends the journey 401 Login failure", async () => {
    const input = tree();
    const bare = {
      ...input,
      scripts: { s: script("httpClient.send('http://invented.example.com/x', {method:'GET'}).get(); action.goTo('true');") },
    };
    const emu = make(bare, { http: () => ({ failure: { kind: "refused" } }) });
    expect(await run(emu, "invented")).toMatchObject({ status: 401, body: { message: "Login failure" } });
  });
  it("a failure kind it does not model is missing", async () => {
    expectEmulationFailure("httpClient mock failure: http://invented.example.com:8080/x");
    const emu = make(tree(), { http: () => ({ failure: { kind: "tls-handshake" } }) as never });
    expect((await run(emu, "invented")).status).toBe(501);
  });
});

// ---------------------------------------------------------------------------------------------
// reCaptchaV3 on product-CaptchaNode (batch 5 B5-1). Doc 25, "CAPTCHA".
describe("product-CaptchaNode reCaptchaV3", () => {
  const captcha = (v3: boolean): JourneyConfigInput => ({
    realm: "alpha",
    trees: {
      invented: {
        entryNodeId: "c",
        nodes: {
          c: { nodeType: "product-CaptchaNode", connections: { true: "t", false: "f" } },
          t: { nodeType: "ScriptedDecisionNode", connections: {} },
          f: { nodeType: "ScriptedDecisionNode", connections: {} },
        },
      },
    },
    nodes: {
      "product-CaptchaNode": {
        c: { siteKey: "invented-site", apiUri: "https://captcha.example.com/api.js", divClass: "invented-div", disableSubmission: false, reCaptchaV3: v3, scoreThreshold: "0.5" },
      },
      ScriptedDecisionNode: { t: { script: "t", outcomes: [] }, f: { script: "f", outcomes: [] } },
    },
    scripts: { t: script(report("true")), f: script(report("false")) },
  });
  type Wire = { callbacks: { output: { name: string; value: JsonValue }[]; input: { value: JsonValue }[] }[] };
  const answer = (body: JsonValue, token: string): JsonValue => {
    const copy = structuredClone(body) as Wire;
    (copy.callbacks[0]!.input[0] as { value: JsonValue }).value = token;
    return copy as unknown as JsonValue;
  };
  it.each([true, false])("sends reCaptchaV3 %s as configured, and the rest of the node's fields", async (v3) => {
    const first = await run(make(captcha(v3)), "invented");
    const fields = Object.fromEntries((first.body as Wire).callbacks[0]!.output.map((f) => [f.name, f.value]));
    // Discriminating: the callback used to say false whatever the node held.
    expect(fields).toEqual({
      recaptchaSiteKey: "invented-site",
      captchaApiUri: "https://captcha.example.com/api.js",
      captchaDivClass: "invented-div",
      reCaptchaV3: v3,
      disableSubmission: false,
    });
  });
  it.each([true, false])("an empty token fails the journey 401 Login failure (reCaptchaV3 %s); a token the provider rejects takes false", async (v3) => {
    const emu = make(captcha(v3), { declaredOutcomes: { c: "false" } });
    const first = await run(emu, "invented");
    expect(await run(emu, "invented", { body: answer(first.body, "") })).toMatchObject({
      status: 401,
      body: { message: "Login failure" },
    });
    // Control: a non-empty token reaches the declared verdict.
    expect(dump((await run(emu, "invented", { body: answer(first.body, "any-token") })).body).branch).toBe("false");
  });
});
