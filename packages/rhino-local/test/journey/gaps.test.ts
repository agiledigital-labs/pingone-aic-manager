/** Journey emulator gaps found by rewriting real journeys; generic trees only. */
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { RhinoRunner } from "../../src/runner.ts";
import { IdmConfig, IdmEmulator } from "../../src/idm/index.ts";
import {
  JourneyConfig,
  JourneyEmulator,
  type JourneyOptions,
} from "../../src/journey/index.ts";
import { expectEmulationFailure } from "../../src/harness/emulation-guard.ts";
import type { JsonValue } from "../../src/protocol.ts";
import type { LogRecord } from "../../src/logs/record.ts";
import {
  SUCCESS_NODE_ID as SUCCESS,
  FAILURE_NODE_ID as FAILURE,
} from "../../src/aic/constants.ts";

let runner: RhinoRunner;
beforeAll(async () => {
  runner = await RhinoRunner.spawn();
}, 60_000);
afterAll(async () => {
  await runner?.close();
});

const b64 = (source: string) => Buffer.from(source).toString("base64");
const decision = (source: string, name = "invented-script") => ({
  name,
  evaluatorVersion: "2.0",
  language: "JAVASCRIPT",
  context: "AUTHENTICATION_TREE_DECISION_NODE",
  script: b64(source),
});
const library = (name: string, source: string) => ({
  name,
  evaluatorVersion: "2.0",
  language: "JAVASCRIPT",
  context: "LIBRARY",
  script: b64(source),
});

/** One scripted node routing true/false to Success/Failure; extra scripts are the libraries. */
function make(
  source: string,
  scripts: Record<string, ReturnType<typeof library>> = {},
  options: JourneyOptions = {},
) {
  return new JourneyEmulator(
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
      nodes: {
        ScriptedDecisionNode: {
          one: { script: "main", outcomes: ["true", "false"] },
        },
      },
      scripts: { main: decision(source), ...scripts },
    }),
    runner,
    new IdmEmulator(
      new IdmConfig({ sync: { mappings: [] }, managed: { objects: [] } }),
      runner,
    ),
    options,
  );
}
const start = (emu: JourneyEmulator, body: Record<string, JsonValue> = {}) =>
  emu.request({
    method: "POST",
    path: "authenticate",
    query: { authIndexType: "service", authIndexValue: "invented" },
    body,
  });

describe("G1: require of library scripts and systemEnv", () => {
  const libs = {
    "id-a": library(
      "lib-a",
      `var b = require("lib-b");
       exports.read = function (k) { return systemEnv.getProperty(k); };
       exports.twice = function (n) { return b.double(n); };`,
    ),
    "id-b": library(
      "lib-b",
      `exports.double = function (n) { return n * 2; };`,
    ),
  };
  const body = (check: string) =>
    `var a = require("lib-a");
     nodeState.putShared("username", "u");
     action.goTo(${check} ? "true" : "false");`;

  it("a library read of systemEnv sees the configured ESV, and a nested require resolves", async () => {
    const emu = make(
      body(
        `a.read("esv.invented.url") === "https://x.example.com" && a.twice(21) === 42`,
      ),
      libs,
      { esv: { "esv.invented.url": "https://x.example.com" } },
    );
    expect((await start(emu)).status).toBe(200);
  });

  it("an ESV that is not configured reads as null, not as a failure (discriminates a stub that throws)", async () => {
    const emu = make(body(`a.read("esv.invented.absent") === null`), libs, {});
    expect((await start(emu)).status).toBe(200);
  });

  it("a decision script reads systemEnv directly", async () => {
    const emu = make(
      `nodeState.putShared("username","u");
       action.goTo(systemEnv.getProperty("esv.invented.flag") === "on" ? "true" : "false");`,
      {},
      { esv: { "esv.invented.flag": "on" } },
    );
    expect((await start(emu)).status).toBe(200);
  });

  it("a library the config does not hold is a failure naming it, never a silent undefined", async () => {
    expectEmulationFailure(/require: library "lib-nowhere" is not declared.*/);
    const emu = make(`require("lib-nowhere"); action.goTo("true");`, {});
    const result = await start(emu);
    expect(result.status).not.toBe(200);
    expect(JSON.stringify(result.body)).toContain("lib-nowhere");
  });

  it("a decision script is not a library: require resolves LIBRARY-context scripts only", async () => {
    expectEmulationFailure(
      /require: library "invented-script" is not declared.*/,
    );
    const emu = make(`require("invented-script"); action.goTo("true");`, {});
    const result = await start(emu);
    expect(result.status).not.toBe(200);
  });
});

describe("G1: ESV placeholders in node configuration", () => {
  const withNode = (
    node: Record<string, unknown>,
    esv?: Record<string, string>,
    type = "RetryLimitDecisionNode",
    wires: Record<string, string> = { Retry: SUCCESS, Reject: FAILURE },
  ) =>
    new JourneyEmulator(
      new JourneyConfig({
        realm: "alpha",
        trees: {
          invented: {
            entryNodeId: "seed",
            nodes: {
              seed: {
                nodeType: "ScriptedDecisionNode",
                connections: { true: "limit" },
              },
              limit: { nodeType: type, connections: wires },
            },
          },
        },
        nodes: {
          ScriptedDecisionNode: {
            seed: { script: "main", outcomes: ["true"] },
          },
          [type]: { limit: node },
        },
        scripts: {
          main: decision(
            `nodeState.putShared("username","u"); action.goTo("true");`,
          ),
        },
      }),
      runner,
      new IdmEmulator(
        new IdmConfig({ sync: { mappings: [] }, managed: { objects: [] } }),
        runner,
      ),
      { now: () => 1_700_000_000_000_000_000n, ...(esv ? { esv } : {}) },
    );
  it("a $int wrapper holding a placeholder reads the configured number", async () => {
    // Limit 0 is refused by the node, so a resolved 5 is the only way this reaches Retry.
    const emu = withNode(
      { retryLimit: { $int: "&{esv.invented.limit}" } },
      { "esv.invented.limit": "5" },
    );
    expect((await start(emu)).status).toBe(200);
  });
  it("a wrapped literal reads as itself", async () => {
    expect((await start(withNode({ retryLimit: { $int: "2" } }))).status).toBe(
      200,
    );
  });
  it("a wrapped signing key resolves to the configured value: same cookie as the literal, another as a different key", async () => {
    const cookieOf = async (
      node: Record<string, unknown>,
      esv?: Record<string, string>,
    ) =>
      (
        await start(
          withNode(node, esv, "SetPersistentCookieNode", { outcome: SUCCESS }),
        )
      ).cookies?.[0]?.value;
    const wrapped = await cookieOf(
      { hmacSigningKey: { $string: "&{esv.invented.key}" } },
      { "esv.invented.key": "key-one" },
    );
    expect(wrapped).toBeDefined();
    expect(wrapped).toBe(await cookieOf({ hmacSigningKey: "key-one" }));
    expect(wrapped).not.toBe(await cookieOf({ hmacSigningKey: "key-two" }));
  });
  it("a placeholder with no configured value is missing, not a default", async () => {
    expectEmulationFailure(
      "ESV placeholder not configured: esv.invented.limit",
    );
    const result = await start(
      withNode({ retryLimit: { $int: "&{esv.invented.limit}" } }),
    );
    expect(result.status).toBe(501);
  });
});

describe("G12: httpClient.send form option", () => {
  const send = async (form: string, extra = "") => {
    const seen: {
      url: string;
      options: Record<string, JsonValue>;
      bodyText: string;
      type?: string;
    }[] = [];
    const emu = make(
      `var r = httpClient.send("https://service.example.com/token", {method: "POST", ${extra} form: ${form}}).get();
       nodeState.putShared("username", "u");
       action.goTo(r.status === 200 ? "true" : "false");`,
      {},
      {
        http: (url, options, wire) => {
          seen.push({
            url,
            options,
            bodyText: wire.bodyText,
            ...(wire.contentType ? { type: wire.contentType } : {}),
          });
          return { status: 200, body: {} };
        },
      },
    );
    return { result: await start(emu), seen };
  };

  it("sends the pairs urlencoded with the content type AIC sets", async () => {
    const { result, seen } = await send(
      `{grant_type: "client_credentials", scope: "a b"}`,
    );
    expect(result.status).toBe(200);
    expect(seen).toHaveLength(1);
    expect(seen[0]).toMatchObject({
      bodyText: "grant_type=client_credentials&scope=a+b",
      type: "application/x-www-form-urlencoded",
    });
  });

  it("encodes as Java's URLEncoder does: the measured characters where a plausible encoder differs", async () => {
    // Measured lengths (docs/api/12 §6): `*` stays literal, `~` is %7E, `%` is %25, space is +.
    const { seen } = await send(
      `{"k e&y": "v=1&z", p: "a+b", s: "*~%41/?#", u: "\\u00e9\\ud83d\\ude00"}`,
    );
    expect(seen[0]?.bodyText).toBe(
      "k+e%26y=v%3D1%26z&p=a%2Bb&s=*%7E%2541%2F%3F%23&u=%C3%A9%F0%9F%98%80",
    );
  });

  it("a form replaces the script's Content-Type and wins over a body", async () => {
    const { seen } = await send(
      `{a: "1"}`,
      `headers: {"Content-Type": "text/plain"}, body: {x: 1},`,
    );
    expect(seen[0]).toMatchObject({
      bodyText: "a=1",
      type: "application/x-www-form-urlencoded",
    });
  });

  it("an empty form is an empty body, still urlencoded", async () => {
    const { seen } = await send(`{}`);
    expect(seen[0]).toMatchObject({
      bodyText: "",
      type: "application/x-www-form-urlencoded",
    });
  });

  it("a number value is refused where AIC throws, never stringified", async () => {
    expectEmulationFailure(
      /httpClient\.send form value that is not a string: n.*/,
    );
    const { result } = await send(`{n: 1}`);
    expect(result.status).toBe(501);
  });
});

describe("G20/G21: a script-built string is a ConsString, not a java.lang.String (docs/api/12 §7)", () => {
  type Seen = {
    options: Record<string, JsonValue>;
    bodyText: string;
    type?: string;
  };
  /** `extra` runs first; `t` is a non-constant string, so `"Bearer " + t` is a concatenation. */
  const run = async (script: string) => {
    const seen: Seen[] = [];
    const emu = make(
      `var t = "ABC".toLowerCase();
       var caught = "none";
       try { ${script} } catch (e) { caught = String(e); }
       nodeState.putShared("username", "u");
       nodeState.putShared("caught", caught);
       action.goTo("true");`,
      {},
      {
        http: (_url, options, wire) => {
          seen.push({
            options,
            bodyText: wire.bodyText,
            ...(wire.contentType ? { type: wire.contentType } : {}),
          });
          return { status: 200, body: {} };
        },
      },
    );
    const result = await start(emu);
    return { result, seen, emu };
  };
  const send = (options: string) =>
    `httpClient.send("https://service.example.com/x", ${options}).get();`;
  /** What the script's own catch saw, read from the success body is not possible; echo it by status. */
  const caughtText = async (options: string) => {
    const seen: Seen[] = [];
    let caught = "";
    const emu = make(
      `var t = "ABC".toLowerCase();
       var caught = "none";
       try { ${send(options)} } catch (e) { caught = String(e); }
       httpClient.send("https://service.example.com/report", {method: "POST", body: {caught: caught}}).get();
       nodeState.putShared("username", "u");
       action.goTo("true");`,
      {},
      {
        http: (url, options, wire) => {
          if (url.endsWith("/report")) {
            caught = String((options.body as { caught: string }).caught);
          } else {
            seen.push({ options, bodyText: wire.bodyText });
          }
          return { status: 200, body: {} };
        },
      },
    );
    expect((await start(emu)).status).toBe(200);
    return { caught, seen };
  };

  it("a header built with + throws the tenant's InternalError at send, and nothing goes out", async () => {
    const { caught, seen } = await caughtText(
      `{method: "POST", headers: {Authorization: "Bearer " + t}}`,
    );
    expect(caught).toBe(
      "InternalError: Cannot add values for key 'Authorization': Bearer abc",
    );
    expect(seen).toHaveLength(0);
  });

  it("the same text as a java.lang.String passes (the discriminating case): toString, concat, String(), slice, join, replace and a literal", async () => {
    for (const value of [
      `("Bearer " + t).toString()`,
      `"Bearer ".concat(t)`,
      `String("Bearer " + t)`,
      `("Bearer " + t).slice(0)`,
      `["Bearer ", t].join("")`,
      `("Bearer " + t).replace("x", "y")`,
      `"Bearer abc"`,
    ]) {
      const { caught, seen } = await caughtText(
        `{method: "POST", headers: {Authorization: ${value}}}`,
      );
      expect(caught, value).toBe("none");
      expect(seen, value).toHaveLength(1);
      expect(seen[0]?.options.headers, value).toEqual({
        Authorization: "Bearer abc",
      });
    }
  });

  it("a number, a boolean, an object and a String object are refused with the value's Java text", async () => {
    const cases: [string, string][] = [
      ["1", "1.0"],
      ["1.5", "1.5"],
      ["1e21", "1.0E21"],
      ["true", "true"],
      ["{a: 1}", "[object Object]"],
      [`new String("abc")`, "abc"],
    ];
    for (const [value, text] of cases) {
      const { caught, seen } = await caughtText(
        `{method: "GET", headers: {"X-Probe": ${value}}}`,
      );
      expect(caught, value).toBe(
        `InternalError: Cannot add values for key 'X-Probe': ${text}`,
      );
      expect(seen, value).toHaveLength(0);
    }
  });

  it("null, undefined and an empty array omit the header and are sent; an array of strings is sent", async () => {
    for (const value of ["null", "undefined", "[]", `["a", "b"]`]) {
      const { caught, seen } = await caughtText(
        `{method: "GET", headers: {"X-Probe": ${value}}}`,
      );
      expect(caught, value).toBe("none");
      expect(seen, value).toHaveLength(1);
    }
  });

  it("an array holding a concatenation is missing behaviour, not a guess at the tenant's class-loader text", async () => {
    expectEmulationFailure(
      /httpClient\.send header array holding a value that is not a plain string: X-Probe.*/,
    );
    const { result } = await run(
      send(`{method: "GET", headers: {"X-Probe": ["a", "b" + t]}}`),
    );
    expect(result.status).toBe(501);
  });

  it("a urlencoded body built with + goes out as a JSON string under application/json, whatever the script's header said", async () => {
    const { seen } = await run(
      send(
        `{method: "POST", headers: {"Content-Type": "application/x-www-form-urlencoded"}, body: "a=1&b=" + t}`,
      ),
    );
    expect(seen).toHaveLength(1);
    expect(seen[0]).toMatchObject({
      bodyText: `"a=1&b=abc"`,
      type: "application/json; charset=UTF-8",
    });
  });

  it("the same body as a java.lang.String is sent raw under the script's own header (the discriminating case)", async () => {
    for (const value of [`("a=1&b=" + t).toString()`, `"a=1&b=abc"`]) {
      const { seen } = await run(
        send(
          `{method: "POST", headers: {"Content-Type": "application/x-www-form-urlencoded"}, body: ${value}}`,
        ),
      );
      expect(seen[0], value).toMatchObject({ bodyText: "a=1&b=abc" });
      expect(seen[0]?.type, value).toBeUndefined();
    }
  });

  it("a concatenated body is escaped as a JSON string, as every measured body string is", async () => {
    const { seen } = await run(
      send(
        `{method: "POST", headers: {"Content-Type": "text/plain"}, body: "q=\\"" + t + "\\\\\\u00e9"}`,
      ),
    );
    expect(seen[0]).toMatchObject({
      // The text q="abc\é as a JSON string: quote and backslash escaped, é raw (docs/api/12 §4).
      bodyText: String.raw`"q=\"abc\\` + "\u00e9" + '"',
      type: "application/json; charset=UTF-8",
    });
  });

  it("an object body under application/json carries the charset suffix; under no header it carries no content type", async () => {
    const withHeader = await run(
      send(
        `{method: "POST", headers: {"Content-Type": "application/json"}, body: {a: 1}}`,
      ),
    );
    expect(withHeader.seen[0]).toMatchObject({
      bodyText: `{"a":1.0}`,
      type: "application/json; charset=UTF-8",
    });
    const without = await run(send(`{method: "POST", body: {a: 1}}`));
    expect(without.seen[0]?.type).toBeUndefined();
  });

  it("a form value or a method built with + is missing behaviour; the toString'd value is fine", async () => {
    expectEmulationFailure(
      /httpClient\.send form value built by concatenation: b.*/,
    );
    const form = await run(
      send(`{method: "POST", form: {a: "1", b: "x" + t}}`),
    );
    expect(form.result.status).toBe(501);
  });

  it("a form value that was toString'd is sent", async () => {
    const { seen } = await run(
      send(`{method: "POST", form: {a: "1", b: ("x" + t).toString()}}`),
    );
    expect(seen[0]).toMatchObject({ bodyText: "a=1&b=xabc" });
  });

  it("a method built with + is missing behaviour", async () => {
    expectEmulationFailure(/httpClient\.send method built by concatenation.*/);
    const { result } = await run(send(`{method: "PO" + t.slice(0, 0) + "ST"}`));
    expect(result.status).toBe(501);
  });
});

describe("G22: a test reads script log lines and audit entries back through logSink", () => {
  const read = async (source: string) => {
    const records: LogRecord[] = [];
    const emu = make(
      `${source}\nnodeState.putShared("username", "u"); action.goTo("true");`,
      {},
      { logSink: { emit: (record) => records.push(record) } },
    );
    return { result: await start(emu), records };
  };
  const payload = (record: LogRecord) =>
    record.payload as Record<string, unknown>;

  it("the script's logger calls come back in order, by level and formatted message, from the records whose logger is the script's", async () => {
    const { result, records } = await read(
      `logger.info("token {} ok", "abc"); logger.warn("slow"); logger.debug("detail");`,
    );
    expect(result.status).toBe(200);
    const lines = records
      .filter((r) => String(payload(r).logger).startsWith("scripts."))
      .map((r) => [payload(r).level, payload(r).message]);
    expect(lines).toEqual([
      ["INFO", "token abc ok"],
      ["WARN", "slow"],
      ["DEBUG", "detail"],
    ]);
  });

  it("an auditEntryDetail trace comes back on the node's AM-NODE-LOGIN-COMPLETED record; a nodeState write does not", async () => {
    const { records } = await read(
      `var auditEntryDetail = {event: "step-done", n: 2};
       nodeState.putShared("auditEntryDetail", "ignored");`,
    );
    const node = records.find(
      (r) => payload(r).eventName === "AM-NODE-LOGIN-COMPLETED",
    );
    const entries = payload(node!).entries as {
      info: { nodeExtraLogging: { auditInfo: unknown } };
    }[];
    expect(entries[0]?.info.nodeExtraLogging.auditInfo).toEqual({
      event: "step-done",
      n: 2,
    });
  });
});

const userStore = () =>
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
                mail: { type: "string", title: "Email Address" },
                givenName: { type: "string", title: "First Name" },
                sn: { type: "string", title: "Last Name" },
                password: { type: "string", title: "Password" },
              },
            },
          },
        ],
      },
    }),
    runner,
  );

/** seed script, then one identity node whose every outcome ends the journey in a state dump. */
function identityFlow(
  type: string,
  node: Record<string, unknown>,
  seed: string,
  idm: IdmEmulator,
  outcomes: string[],
) {
  const dump = (tag: string) =>
    `if (callbacks.isEmpty()) {
       callbacksBuilder.hiddenValueCallback("r", JSON.stringify({tag: "${tag}", attrs: nodeState.get("objectAttributes"), username: nodeState.get("username")}));
     } else { action.goTo("done"); }`;
  return new JourneyEmulator(
    new JourneyConfig({
      realm: "alpha",
      trees: {
        invented: {
          entryNodeId: "seed",
          nodes: {
            seed: {
              nodeType: "ScriptedDecisionNode",
              connections: { next: "node" },
            },
            node: {
              nodeType: type,
              connections: Object.fromEntries(
                outcomes.map((o) => [o, `dump-${o}`]),
              ),
            },
            ...Object.fromEntries(
              outcomes.map((o) => [
                `dump-${o}`,
                {
                  nodeType: "ScriptedDecisionNode",
                  connections: { done: `after-${o}` },
                },
              ]),
            ),
            ...Object.fromEntries(
              outcomes.map((o) => [
                `after-${o}`,
                {
                  nodeType: "ScriptedDecisionNode",
                  connections: { done: SUCCESS },
                },
              ]),
            ),
          },
        },
      },
      nodes: {
        [type]: { node },
        ScriptedDecisionNode: {
          seed: { script: "seed", outcomes: ["next"] },
          ...Object.fromEntries(
            outcomes.flatMap((o) => [
              [`dump-${o}`, { script: `dump-${o}`, outcomes: ["done"] }],
              [`after-${o}`, { script: `after-${o}`, outcomes: ["done"] }],
            ]),
          ),
        },
      },
      scripts: {
        seed: decision(`${seed}; action.goTo("next");`),
        ...Object.fromEntries(
          outcomes.flatMap((o) => [
            [`dump-${o}`, decision(dump(o))],
            [`after-${o}`, decision(dump(`after-${o}`))],
          ]),
        ),
      },
    }),
    runner,
    idm,
  );
}
const dumpOf = (
  body: unknown,
): { tag: string; attrs: JsonValue; username: JsonValue } =>
  JSON.parse(
    String(
      (
        body as {
          callbacks: { output: { name: string; value: JsonValue }[] }[];
        }
      ).callbacks[0]?.output.find((field) => field.name === "value")?.value,
    ),
  ) as { tag: string; attrs: JsonValue; username: JsonValue };
const post = (emu: JourneyEmulator, body: JsonValue = {}) =>
  emu.request({
    method: "POST",
    path: "authenticate",
    query: { authIndexType: "service", authIndexValue: "invented" },
    body,
  });
const UID = "5b1f4c0e-7d5a-4c8e-9a3b-0c2d4e6f8a10";
const row = async (idm: IdmEmulator, userName: string) => {
  const found = await idm.request({
    method: "GET",
    path: "managed/alpha_user",
    query: { _queryFilter: `userName eq "${userName}"` },
  });
  return (found.body as { result: Record<string, JsonValue>[] }).result[0];
};

describe("G11: objectAttributes split across shared and transient", () => {
  const create = { identityResource: "managed/alpha_user" };
  const patch = {
    ...create,
    identityAttribute: "userName",
    patchAsObject: false,
    ignoredFields: [],
  };
  const identify = { identityAttribute: "mail", identifier: "userName" };

  it("Create reads the shared map overlaid by the transient one, and neither bucket changes", async () => {
    const idm = userStore();
    const emu = identityFlow(
      "CreateObjectNode",
      create,
      `nodeState.putShared("objectAttributes", {userName: "c1", givenName: "G", sn: "SharedSn", mail: "c1@example.com"});
       nodeState.putTransient("objectAttributes", {sn: "TransientSn", password: "Kdemo-Pw-1x!"});`,
      idm,
      ["CREATED", "FAILURE"],
    );
    const first = await post(emu);
    // Transient-first lookup alone would hold only {sn, password}; the record has all five.
    expect(dumpOf(first.body)).toMatchObject({ tag: "CREATED" });
    const record = await row(idm, "c1");
    expect(record).toMatchObject({
      givenName: "G",
      sn: "TransientSn",
      mail: "c1@example.com",
    });
    // After a round trip only shared remains, exactly as seeded: Create wrote no state.
    const second = await post(emu, first.body as JsonValue);
    expect(dumpOf(second.body)).toMatchObject({
      tag: "after-CREATED",
      attrs: {
        userName: "c1",
        givenName: "G",
        sn: "SharedSn",
        mail: "c1@example.com",
      },
    });
  });

  it("Patch applies both buckets, finding the user from either", async () => {
    const idm = userStore();
    await idm.request({
      method: "PUT",
      path: `managed/alpha_user/${UID}`,
      body: {
        userName: "p1",
        password: "Passw0rd!x",
        givenName: "Orig",
        sn: "Orig",
        mail: "p1@example.com",
      },
    });
    const emu = identityFlow(
      "PatchObjectNode",
      patch,
      `nodeState.putShared("objectAttributes", {sn: "SharedSn"});
       nodeState.putTransient("objectAttributes", {userName: "p1", givenName: "TransientGn"});`,
      idm,
      ["PATCHED", "FAILURE"],
    );
    expect(dumpOf((await post(emu)).body)).toMatchObject({ tag: "PATCHED" });
    expect(await row(idm, "p1")).toMatchObject({
      givenName: "TransientGn",
      sn: "SharedSn",
    });
  });

  it("Identify looks up the transient value when both buckets hold the key, and adds userName to shared only", async () => {
    const idm = userStore();
    await idm.request({
      method: "PUT",
      path: `managed/alpha_user/${UID}`,
      body: { userName: "i1", password: "Passw0rd!x", mail: "i1@example.com" },
    });
    const hit = identityFlow(
      "IdentifyExistingUserNode",
      identify,
      `nodeState.putShared("objectAttributes", {mail: "nobody@example.com", sn: "kept"});
       nodeState.putTransient("objectAttributes", {mail: "i1@example.com"});`,
      idm,
      ["true", "false"],
    );
    const first = await post(hit);
    expect(dumpOf(first.body)).toMatchObject({ tag: "true", username: "i1" });
    // Shared keeps its own mail and sn and gains userName; the lookup used the transient mail.
    const second = await post(hit, first.body as JsonValue);
    expect(dumpOf(second.body)).toMatchObject({
      tag: "after-true",
      attrs: { mail: "nobody@example.com", sn: "kept", userName: "i1" },
    });
    // The reverse: a shared hit under a transient miss is a miss, so the transient value decides.
    const miss = identityFlow(
      "IdentifyExistingUserNode",
      identify,
      `nodeState.putShared("objectAttributes", {mail: "i1@example.com"});
       nodeState.putTransient("objectAttributes", {mail: "nobody@example.com"});`,
      idm,
      ["true", "false"],
    );
    expect(dumpOf((await post(miss)).body)).toMatchObject({ tag: "false" });
  });

  it("a key both buckets hold as an object is missing, not guessed", async () => {
    expectEmulationFailure(
      "split objectAttributes sharing a nested object key",
    );
    const emu = identityFlow(
      "CreateObjectNode",
      create,
      `nodeState.putShared("objectAttributes", {userName: "n1", prefs: {a: "1"}});
       nodeState.putTransient("objectAttributes", {prefs: {b: "2"}});`,
      userStore(),
      ["CREATED", "FAILURE"],
    );
    expect((await post(emu)).status).toBe(501);
  });
});

/** A Page of the given children leading to a dump script that reports shared `cnt` in a hidden callback. */
const TERMS = {
  version: "9.1",
  terms: "Invented terms.",
  createDate: "2020-01-02T03:04:05.678Z",
};
function pageFlow(
  kids: [string, string, Record<string, JsonValue>][],
  page: Record<string, JsonValue>,
  outcomes: string[],
  options: JourneyOptions = {},
  scripts: Record<string, ReturnType<typeof decision>> = {},
) {
  const nodes: Record<string, Record<string, Record<string, JsonValue>>> = {
    PageNode: {
      pg: {
        nodes: kids.map(([id, nodeType]) => ({
          _id: id,
          nodeType,
          displayName: id,
        })),
        ...page,
      },
    },
    ScriptedDecisionNode: { dump: { script: "dump", outcomes: ["true"] } },
  };
  for (const [id, type, config] of kids) (nodes[type] ??= {})[id] = config;
  return new JourneyEmulator(
    new JourneyConfig({
      realm: "alpha",
      trees: {
        invented: {
          entryNodeId: "pg",
          nodes: {
            pg: {
              nodeType: "PageNode",
              connections: Object.fromEntries(outcomes.map((o) => [o, "dump"])),
            },
            dump: {
              nodeType: "ScriptedDecisionNode",
              connections: { true: SUCCESS },
            },
          },
        },
      },
      nodes,
      scripts: {
        dump: decision(
          `if (callbacks.isEmpty()) { callbacksBuilder.hiddenValueCallback("r", "cnt=" + nodeState.get("cnt") + ";via=" + nodeState.get("via")); } else { action.goTo("true"); }`,
        ),
        ...scripts,
      },
    }),
    runner,
    new IdmEmulator(
      new IdmConfig({ sync: { mappings: [] }, managed: { objects: [] } }),
      runner,
    ),
    options,
  );
}
const hiddenOf = (body: JsonValue): string => {
  const cb = (
    body as { callbacks: { output: { name: string; value: string }[] }[] }
  ).callbacks[0];
  return cb?.output.find((o) => o.name === "value")?.value ?? "";
};
const answerAll = (body: JsonValue, values: JsonValue[]): JsonValue => {
  const copy = structuredClone(body) as {
    callbacks: { input: { value: JsonValue }[] }[];
  };
  copy.callbacks.forEach((cb, i) => {
    if (cb.input[0] && i in values) cb.input[0].value = values[i] as JsonValue;
  });
  return copy as unknown as JsonValue;
};
const COUNT = decision(
  `var c = nodeState.get("cnt"); nodeState.putShared("cnt", String((c === null ? 0 : parseInt(c)) + 1)); action.goTo("true");`,
  "count",
);
const COUNT_ONLY = { cnt: COUNT };

describe("G7: a page with no callbacks", () => {
  it("runs its scripted child once and leaves by that child's outcome", async () => {
    const emu = pageFlow(
      [["s", "ScriptedDecisionNode", { script: "cnt", outcomes: ["true"] }]],
      {},
      ["true"],
      {},
      COUNT_ONLY,
    );
    const first = await post(emu);
    // Run once (a double discovery-and-submit run would read cnt=2), nothing page-shaped sent.
    expect(hiddenOf(first.body as JsonValue)).toBe("cnt=1;via=null");
  });
});

describe("G2: PageNode header and description", () => {
  const user = ["u", "UsernameCollectorNode", {}] as [
    string,
    string,
    Record<string, JsonValue>,
  ];
  const run = async (page: Record<string, JsonValue>) =>
    (await post(pageFlow([user], page, ["outcome"]))).body as Record<
      string,
      JsonValue
    >;
  it("sends the en entry as header and description beside the callbacks", async () => {
    const body = await run({
      pageHeader: { en: "E", fr: "F" },
      pageDescription: { en: "D" },
    });
    expect(body).toMatchObject({ header: "E", description: "D" });
    expect(Object.keys(body)).toEqual([
      "authId",
      "callbacks",
      "header",
      "description",
    ]);
  });
  it("uses a sole non-en entry and omits an empty one", async () => {
    expect(
      await run({ pageHeader: { fr: "F" }, pageDescription: { en: "" } }),
    ).toMatchObject({ header: "F" });
    const body = await run({
      pageHeader: { fr: "F" },
      pageDescription: { en: "" },
    });
    expect(body).not.toHaveProperty("description");
    expect(
      await run({ pageHeader: {}, pageDescription: {} }),
    ).not.toHaveProperty("header");
  });
  it("repeats them when a rejected page is re-sent", async () => {
    const terms = ["t", "AcceptTermsAndConditionsNode", {}] as [
      string,
      string,
      Record<string, JsonValue>,
    ];
    const emu = pageFlow(
      [user, terms],
      { pageHeader: { en: "E" } },
      ["outcome"],
      { termsAndConditions: TERMS },
    );
    const first = await post(emu);
    const again = await post(
      emu,
      answerAll(first.body as JsonValue, ["u", false]),
    );
    expect(again.body).toMatchObject({ header: "E" });
  });
  it("several locales with no en is missing", async () => {
    expectEmulationFailure(
      "PageNode pageHeader with several locales and no en",
    );
    expect(
      (
        await post(
          pageFlow([user], { pageHeader: { fr: "F", de: "D" } }, ["outcome"]),
        )
      ).status,
    ).toBe(501);
  });
});

describe("G9: Terms and Conditions as a page child", () => {
  const user = ["u", "UsernameCollectorNode", {}] as [
    string,
    string,
    Record<string, JsonValue>,
  ];
  const terms = ["t", "AcceptTermsAndConditionsNode", {}] as [
    string,
    string,
    Record<string, JsonValue>,
  ];
  it("sends its callback after the collector's and leaves once accepted", async () => {
    const emu = pageFlow([user, terms], {}, ["outcome"], {
      termsAndConditions: TERMS,
    });
    const first = await post(emu);
    expect(
      (first.body as { callbacks: { type: string }[] }).callbacks.map(
        (c) => c.type,
      ),
    ).toEqual(["NameCallback", "TermsAndConditionsCallback"]);
    const done = await post(
      emu,
      answerAll(first.body as JsonValue, ["u", true]),
    );
    expect(hiddenOf(done.body as JsonValue)).toBe("cnt=null;via=null");
  });
  it("re-sends the page, keeping the entered name, when declined", async () => {
    const emu = pageFlow([user, terms], {}, ["outcome"], {
      termsAndConditions: TERMS,
    });
    const first = await post(emu);
    const again = await post(
      emu,
      answerAll(first.body as JsonValue, ["u", false]),
    );
    const cbs = (
      again.body as {
        callbacks: { type: string; input: { value: JsonValue }[] }[];
      }
    ).callbacks;
    expect(cbs.map((c) => c.type)).toEqual([
      "NameCallback",
      "TermsAndConditionsCallback",
    ]);
    expect(cbs[0]?.input[0]?.value).toBe("u");
    // Review round 3 P2-3: the re-send carries the declined terms child's own evidence.
    const behaviors = (kind: string) =>
      (again.provenance ?? []).filter((e) => e.kind === kind).map((e) => e.behavior).join("\n");
    expect(behaviors("measured")).toContain("a declined terms child re-sends the whole page");
    expect(behaviors("measured")).not.toContain("failedPolicies");
    expect(behaviors("assumed")).not.toContain("echoes an accepted sibling");
  });
});

describe("G3: product-CaptchaNode as the last page child", () => {
  const user = ["u", "UsernameCollectorNode", {}] as [
    string,
    string,
    Record<string, JsonValue>,
  ];
  const captcha = [
    "c",
    "product-CaptchaNode",
    {
      siteKey: "k",
      secretKey: "s",
      apiUri: "https://captcha.example.com/api.js",
      captchaUri: "https://captcha.example.com/verify",
      divClass: "g-recaptcha",
      disableSubmission: false,
      reCaptchaV3: false,
    },
  ] as [string, string, Record<string, JsonValue>];
  it("sends the captcha callback and leaves by the declared verdict", async () => {
    for (const verdict of ["true", "false"]) {
      const emu = pageFlow([user, captcha], {}, ["true", "false"], {
        declaredOutcomes: { c: verdict },
      });
      const first = await post(emu);
      expect(
        (first.body as { callbacks: { type: string }[] }).callbacks.map(
          (c) => c.type,
        ),
      ).toEqual(["NameCallback", "ReCaptchaCallback"]);
      const done = await post(
        emu,
        answerAll(first.body as JsonValue, ["u", "token"]),
      );
      expect(hiddenOf(done.body as JsonValue)).toBe("cnt=null;via=null");
    }
  });
  it("an empty token fails the journey with 401", async () => {
    const emu = pageFlow([user, captcha], {}, ["true", "false"], {
      declaredOutcomes: { c: "true" },
    });
    const first = await post(emu);
    const done = await post(emu, answerAll(first.body as JsonValue, ["u", ""]));
    expect(done.status).toBe(401);
  });
});

describe("G10: ValidatedPasswordNode with validateInput true", () => {
  const POLICY = {
    minLength: 8,
    characterSets: [
      "0:0123456789",
      "0:ABCDEFGHIJKLMNOPQRSTUVWXYZ",
      "0:abcdefghijklmnopqrstuvwxyz",
    ],
    minCharacterSets: 3,
  };
  const vp = (options: JourneyOptions = { passwordPolicy: POLICY }) =>
    new JourneyEmulator(
      new JourneyConfig({
        realm: "alpha",
        trees: {
          invented: {
            entryNodeId: "vp",
            nodes: {
              vp: {
                nodeType: "ValidatedPasswordNode",
                connections: { outcome: "dump" },
              },
              dump: {
                nodeType: "ScriptedDecisionNode",
                connections: { true: SUCCESS },
              },
            },
          },
        },
        nodes: {
          ValidatedPasswordNode: {
            vp: { passwordAttribute: "password", validateInput: true },
          },
          ScriptedDecisionNode: {
            dump: { script: "dump", outcomes: ["true"] },
          },
        },
        scripts: {
          dump: decision(
            `if (callbacks.isEmpty()) { callbacksBuilder.hiddenValueCallback("r", String(nodeState.get("password")) + "|" + JSON.stringify(nodeState.get("objectAttributes"))); } else { action.goTo("true"); }`,
          ),
        },
      }),
      runner,
      new IdmEmulator(
        new IdmConfig({ sync: { mappings: [] }, managed: { objects: [] } }),
        runner,
      ),
      options,
    );
  const submit = async (
    emu: JourneyEmulator,
    password: string,
    validateOnly = false,
  ) => {
    const first = await post(emu);
    const body = structuredClone(first.body) as {
      callbacks: { input: { value: JsonValue }[] }[];
    };
    const input = body.callbacks[0]?.input;
    if (!input?.[0] || !input[1]) throw new Error("callback input shape");
    input[0].value = password;
    input[1].value = validateOnly;
    return { first, next: await post(emu, body as unknown as JsonValue) };
  };
  const failedOf = (body: JsonValue): string[] =>
    JSON.parse(
      JSON.stringify(
        (
          body as {
            callbacks: { output: { name: string; value: JsonValue }[] }[];
          }
        ).callbacks[0]?.output.find((o) => o.name === "failedPolicies")?.value,
      ),
    ).map(
      (raw: string) =>
        (JSON.parse(raw) as { policyRequirement: string }).policyRequirement,
    );

  it("sends a ValidatedCreatePasswordCallback with the measured shape", async () => {
    const first = await post(vp());
    expect(first.body).toMatchObject({
      callbacks: [
        {
          type: "ValidatedCreatePasswordCallback",
          output: [
            { name: "echoOn", value: false },
            {
              name: "policies",
              value: { policyRequirements: ["VALID_TYPE"], name: "password" },
            },
            { name: "failedPolicies", value: [] },
            { name: "validateOnly", value: false },
            { name: "prompt", value: "Password" },
          ],
          input: [
            { name: "IDToken1", value: "" },
            { name: "IDToken1validateOnly", value: false },
          ],
        },
      ],
    });
  });
  // Each case differs from its neighbours in exactly the requirement it breaks.
  it.each([
    ["abc", ["CHARACTER_SET", "LENGTH_BASED"]],
    ["", ["CHARACTER_SET", "LENGTH_BASED"]],
    ["abcdefgh1", ["CHARACTER_SET"]],
    ["Aa1", ["LENGTH_BASED"]],
  ])(
    "a rejected password %j sends the page again naming %j",
    async (password, failed) => {
      const { next } = await submit(vp(), password);
      expect(failedOf(next.body as JsonValue)).toEqual(failed);
      // The rejected password is never echoed back.
      expect(
        (next.body as { callbacks: { input: { value: JsonValue }[] }[] })
          .callbacks[0]?.input[0]?.value,
      ).toBe("");
    },
  );
  it("accepts a password that meets the policy and writes transient state", async () => {
    const { next } = await submit(vp(), "Aa1Aa1Aa1");
    expect(hiddenOf(next.body as JsonValue)).toBe(
      'Aa1Aa1Aa1|{"password":"Aa1Aa1Aa1"}',
    );
  });
  it("a validate-only submission is sent again with validateOnly true and writes nothing", async () => {
    const good = await submit(vp(), "Aa1Aa1Aa1", true);
    expect(failedOf(good.next.body as JsonValue)).toEqual([]);
    expect(JSON.stringify(good.next.body)).toContain(
      '"name":"validateOnly","value":true',
    );
    const bad = await submit(vp(), "abc", true);
    expect(failedOf(bad.next.body as JsonValue)).toEqual([
      "CHARACTER_SET",
      "LENGTH_BASED",
    ]);
  });
  it("needs the realm password policy", async () => {
    expectEmulationFailure(
      "ValidatedPasswordNode validateInput: true needs JourneyOptions.passwordPolicy",
    );
    expect((await post(vp({}))).status).toBe(501);
  });
});

describe("G6: slash-path attribute names", () => {
  const collector = (attributes: string[]) =>
    new JourneyEmulator(
      new JourneyConfig({
        realm: "alpha",
        trees: {
          invented: {
            entryNodeId: "at",
            nodes: {
              at: {
                nodeType: "AttributeCollectorNode",
                connections: { outcome: "dump" },
              },
              dump: {
                nodeType: "ScriptedDecisionNode",
                connections: { true: SUCCESS },
              },
            },
          },
        },
        nodes: {
          AttributeCollectorNode: {
            at: {
              identityResource: "managed/widget",
              attributesToCollect: attributes,
              validateInputs: false,
              required: false,
            },
          },
          ScriptedDecisionNode: {
            dump: { script: "dump", outcomes: ["true"] },
          },
        },
        scripts: {
          dump: decision(
            `if (callbacks.isEmpty()) { callbacksBuilder.hiddenValueCallback("r", JSON.stringify(nodeState.get("objectAttributes"))); } else { action.goTo("true"); }`,
          ),
        },
      }),
      runner,
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
                    flag: { type: "boolean", title: "Flag" },
                    preferences: {
                      type: "object",
                      properties: {
                        otp: { type: "string", title: "One-time code" },
                        remember: {
                          type: "boolean",
                          title: "Trust this device",
                        },
                        attempts: { type: "number", title: "Attempts" },
                      },
                    },
                  },
                },
              },
            ],
          },
        }),
        runner,
      ),
    );
  it("looks the nested schema up, names the callback by its path and stores nested, keys sorted", async () => {
    const emu = collector(["preferences/otp", "givenName"]);
    const first = await post(emu);
    const cbs = (
      first.body as {
        callbacks: { output: { name: string; value: JsonValue }[] }[];
      }
    ).callbacks;
    expect(
      cbs.map((cb) => cb.output.find((o) => o.name === "name")?.value),
    ).toEqual(["preferences/otp", "givenName"]);
    expect(cbs[0]?.output.find((o) => o.name === "prompt")?.value).toBe(
      "One-time code",
    );
    const done = await post(
      emu,
      answerAll(first.body as JsonValue, ["123456", "Gv"]),
    );
    // givenName sorts before preferences although it was collected second; the flat key is never written.
    expect(hiddenOf(done.body as JsonValue)).toBe(
      '{"givenName":"Gv","preferences":{"otp":"123456"}}',
    );
  });
  it("a path through a property that is not an object is missing", async () => {
    expectEmulationFailure("AttributeCollectorNode schema: givenName/otp");
    expect((await post(collector(["givenName/otp"]))).status).toBe(501);
  });
  it("a slash path that ends on a type with no callback modelled is missing", async () => {
    expectEmulationFailure(
      "AttributeCollectorNode string or boolean schema title: preferences/attempts",
    );
    expect((await post(collector(["preferences/attempts"]))).status).toBe(501);
  });
});

describe("G4: inner trees that wait for the user, and nest", () => {
  const read = (tag: string) =>
    decision(
      `nodeState.putShared("r_${tag}", JSON.stringify({name: String(journey.name()), inner: String(journey.innerJourney()), os: String(nodeState.get("os")), ot: String(nodeState.get("ot")), t1: String(nodeState.get("t1")), t2: String(nodeState.get("t2")), user: String(nodeState.get("username"))})); action.goTo("true");`,
      `read-${tag}`,
    );
  const put = (name: string, shared: string, transient: string) =>
    decision(
      `nodeState.putShared("${shared}", "s"); nodeState.putTransient("${transient}", "t"); action.goTo("true");`,
      name,
    );
  const build = (withCollector: boolean) =>
    new JourneyEmulator(
      new JourneyConfig({
        realm: "alpha",
        trees: {
          invented: {
            entryNodeId: "seedO",
            nodes: {
              seedO: {
                nodeType: "ScriptedDecisionNode",
                connections: { true: "e1" },
              },
              e1: {
                nodeType: "InnerTreeEvaluatorNode",
                connections: { true: "dump", false: FAILURE },
              },
              dump: {
                nodeType: "ScriptedDecisionNode",
                connections: { true: SUCCESS },
              },
            },
          },
          one: {
            entryNodeId: "seed1",
            innerTreeOnly: true,
            nodes: {
              seed1: {
                nodeType: "ScriptedDecisionNode",
                connections: { true: "a" },
              },
              a: {
                nodeType: "ScriptedDecisionNode",
                connections: { true: "e2" },
              },
              e2: {
                nodeType: "InnerTreeEvaluatorNode",
                connections: { true: "b", false: FAILURE },
              },
              b: {
                nodeType: "ScriptedDecisionNode",
                connections: { true: SUCCESS },
              },
            },
          },
          two: {
            entryNodeId: "seed2",
            innerTreeOnly: true,
            nodes: {
              seed2: {
                nodeType: "ScriptedDecisionNode",
                connections: { true: "c" },
              },
              c: {
                nodeType: "ScriptedDecisionNode",
                connections: { true: withCollector ? "user" : "d" },
              },
              user: {
                nodeType: "UsernameCollectorNode",
                connections: { outcome: "d" },
              },
              d: {
                nodeType: "ScriptedDecisionNode",
                connections: { true: SUCCESS },
              },
            },
          },
        },
        nodes: {
          InnerTreeEvaluatorNode: { e1: { tree: "one" }, e2: { tree: "two" } },
          UsernameCollectorNode: { user: {} },
          ScriptedDecisionNode: Object.fromEntries(
            ["seedO", "seed1", "seed2", "a", "b", "c", "d", "dump"].map(
              (id) => [id, { script: id, outcomes: ["true"] }],
            ),
          ),
        },
        scripts: {
          seedO: put("seedO", "os", "ot"),
          seed1: put("seed1", "s1", "t1"),
          seed2: put("seed2", "s2", "t2"),
          a: read("a"),
          b: read("b"),
          c: read("c"),
          d: read("d"),
          dump: decision(
            `if (callbacks.isEmpty()) { callbacksBuilder.hiddenValueCallback("r", JSON.stringify({a: nodeState.get("r_a"), b: nodeState.get("r_b"), c: nodeState.get("r_c"), d: nodeState.get("r_d"), ot: String(nodeState.get("ot")), name: String(journey.name())})); } else { action.goTo("true"); }`,
          ),
        },
      }),
      runner,
      new IdmEmulator(
        new IdmConfig({ sync: { mappings: [] }, managed: { objects: [] } }),
        runner,
      ),
    );
  const report = (body: JsonValue) =>
    Object.fromEntries(
      Object.entries(JSON.parse(hiddenOf(body)) as Record<string, string>).map(
        ([k, v]) => [
          k,
          typeof v === "string" && v.startsWith("{")
            ? (JSON.parse(v) as JsonValue)
            : v,
        ],
      ),
    ) as Record<string, Record<string, string> | string>;
  it("nests: shared visible at every depth, transient isolated and restored on return", async () => {
    const result = report((await post(build(false))).body as JsonValue);
    expect(result).toMatchObject({
      ot: "t",
      name: "invented",
      // Inner 1 sees only its own transient; inner 2 sees neither of the outer ones.
      a: {
        name: "one",
        inner: "true",
        os: "s",
        ot: "null",
        t1: "t",
        t2: "null",
      },
      c: {
        name: "two",
        inner: "true",
        os: "s",
        ot: "null",
        t1: "null",
        t2: "t",
      },
      // Back in inner 1 its transient is restored, and inner 2's shared writes remain.
      b: { name: "one", t1: "t", t2: "null" },
    });
  });
  it("resumes inside the inner tree after a callback, which drops transient at every level", async () => {
    const emu = build(true);
    const first = await post(emu);
    expect(
      (first.body as { callbacks: { type: string }[] }).callbacks[0]?.type,
    ).toBe("NameCallback");
    const done = await post(
      emu,
      answerAll(first.body as JsonValue, ["someone"]),
    );
    const result = report(done.body as JsonValue);
    expect(result).toMatchObject({
      ot: "null",
      name: "invented",
      d: { name: "two", inner: "true", t2: "null", user: "someone" },
      b: { name: "one", t1: "null", user: "someone" },
    });
  });
});

describe("G8: success with no identified user is tenant behaviour", () => {
  it("answers an empty 500 when Success is reached with no username, as the tenant does", async () => {
    const emu = make(`action.goTo("true");`);
    const none = await post(emu);
    expect(none.status).toBe(500);
    expect(none.body).toBeNull();
    // The control: the same tree with a shared username succeeds.
    const some = await post(
      make(`nodeState.putShared("username", "someone"); action.goTo("true");`),
    );
    expect(some.status).toBe(200);
    expect(some.body).toMatchObject({ successUrl: expect.any(String) });
  });
});

describe("G17: AttributeCollectorNode validateInputs: true", () => {
  const POLICIES = {
    mail: {
      policies: {
        name: "mail",
        policyRequirements: ["REQUIRED", "VALID_EMAIL_ADDRESS_FORMAT"],
      },
      requirements: ["REQUIRED", "VALID_EMAIL_ADDRESS_FORMAT"],
    },
    name: {
      policies: { name: "name", policyRequirements: ["REQUIRED"] },
      requirements: ["REQUIRED"],
    },
  } as const;
  const build = (
    config: Record<string, unknown>,
    options: JourneyOptions = { attributePolicies: POLICIES as never },
  ) =>
    new JourneyEmulator(
      new JourneyConfig({
        realm: "alpha",
        trees: {
          invented: {
            entryNodeId: "at",
            nodes: {
              at: {
                nodeType: "AttributeCollectorNode",
                connections: { outcome: "dump" },
              },
              dump: {
                nodeType: "ScriptedDecisionNode",
                connections: { true: SUCCESS },
              },
            },
          },
        },
        nodes: {
          AttributeCollectorNode: {
            at: {
              identityResource: "managed/widget",
              attributesToCollect: ["name", "mail"],
              validateInputs: true,
              required: true,
              ...config,
            },
          },
          ScriptedDecisionNode: {
            dump: { script: "dump", outcomes: ["true"] },
          },
        },
        scripts: {
          dump: decision(
            `if (callbacks.isEmpty()) { callbacksBuilder.hiddenValueCallback("r", JSON.stringify(nodeState.get("objectAttributes"))); } else { action.goTo("true"); }`,
          ),
        },
      }),
      runner,
      new IdmEmulator(
        new IdmConfig({
          sync: { mappings: [] },
          managed: {
            objects: [
              {
                name: "widget",
                schema: {
                  properties: {
                    name: { type: "string", title: "Name" },
                    mail: { type: "string", title: "Email" },
                  },
                },
              },
            ],
          },
        }),
        runner,
      ),
      options,
    );
  type Out = { name: string; value: JsonValue };
  const outputs = (
    body: JsonValue,
  ): Record<string, Record<string, JsonValue>> => {
    const result: Record<string, Record<string, JsonValue>> = {};
    for (const cb of (body as { callbacks: { output: Out[] }[] }).callbacks) {
      const out = Object.fromEntries(cb.output.map((o) => [o.name, o.value]));
      result[String(out.name)] = out;
    }
    return result;
  };
  const submit = (
    body: JsonValue,
    values: string[],
    validateOnly: number[] = [],
  ): JsonValue => {
    const copy = structuredClone(body) as {
      callbacks: { input: { value: JsonValue }[] }[];
    };
    copy.callbacks.forEach((cb, i) => {
      if (cb.input[0]) cb.input[0].value = values[i] as string;
      if (cb.input[1]) cb.input[1].value = validateOnly.includes(i);
    });
    return copy as unknown as JsonValue;
  };

  it("sends the supplied policies with each attribute", async () => {
    const first = outputs((await post(build({}))).body as JsonValue);
    expect(first.mail?.policies).toEqual(POLICIES.mail.policies);
    expect(first.name?.failedPolicies).toEqual([]);
  });
  it("a malformed address is sent back with its answers kept and the failure as JSON text; a good one proceeds", async () => {
    const emu = build({});
    const first = await post(emu);
    const retry = await post(
      emu,
      submit(first.body as JsonValue, ["Zz", "not-an-email"]),
    );
    const out = outputs(retry.body as JsonValue);
    // Discriminating: an implementation that ignores the policy would have left the node here.
    expect(out.mail).toMatchObject({
      value: "not-an-email",
      failedPolicies: ['{ "policyRequirement": "VALID_EMAIL_ADDRESS_FORMAT" }'],
    });
    expect(out.name).toMatchObject({ value: "Zz", failedPolicies: [] });
    const ok = await post(
      emu,
      submit(retry.body as JsonValue, ["Zz", "a@example.com"]),
    );
    expect(hiddenOf(ok.body as JsonValue)).toBe(
      '{"mail":"a@example.com","name":"Zz"}',
    );
  });
  it("the same bad address is accepted with validateInputs false (control)", async () => {
    const emu = build({ validateInputs: false });
    const first = await post(emu);
    const ok = await post(
      emu,
      submit(first.body as JsonValue, ["Zz", "not-an-email"]),
    );
    expect(hiddenOf(ok.body as JsonValue)).toBe(
      '{"mail":"not-an-email","name":"Zz"}',
    );
  });
  it("an empty answer fails REQUIRED only on a node that is not itself required", async () => {
    // required: false -> the policy names REQUIRED; required: true -> retry with no failed policy.
    const loose = build({ required: false });
    const a = await post(loose);
    const looseOut = outputs(
      (await post(loose, submit(a.body as JsonValue, ["Zz", ""])))
        .body as JsonValue,
    );
    expect(looseOut.mail?.failedPolicies).toEqual([
      '{ "policyRequirement": "REQUIRED" }',
    ]);
    const strict = build({});
    const b = await post(strict);
    const strictOut = outputs(
      (await post(strict, submit(b.body as JsonValue, ["Zz", ""])))
        .body as JsonValue,
    );
    expect(strictOut.mail?.failedPolicies).toEqual([]);
    expect(strictOut.mail?.value).toBe("");
  });
  it("a validate-only submission of a bad value is sent again with validateOnly true", async () => {
    const emu = build({});
    const first = await post(emu);
    const out = outputs(
      (await post(emu, submit(first.body as JsonValue, ["Zz", "bad"], [1])))
        .body as JsonValue,
    );
    expect(out.mail).toMatchObject({
      validateOnly: true,
      failedPolicies: expect.any(Array),
    });
    expect(out.name?.validateOnly).toBe(false);
  });
  it("validate-only with nothing failing is unmeasured: missing", async () => {
    expectEmulationFailure(
      "AttributeCollectorNode validateOnly with nothing failing",
    );
    const emu = build({});
    const first = await post(emu);
    expect(
      (
        await post(
          emu,
          submit(first.body as JsonValue, ["Zz", "a@example.com"], [1]),
        )
      ).status,
    ).toBe(501);
  });
  it("without attributePolicies it is missing, and so is an attribute with no policy", async () => {
    expectEmulationFailure(
      "AttributeCollectorNode validateInputs: true needs JourneyOptions.attributePolicies",
    );
    expect((await post(build({}, {}))).status).toBe(501);
  });
  it("a requirement the emulator does not model is refused, not ignored", async () => {
    expectEmulationFailure(
      "AttributeCollectorNode policy requirement: MIN_LENGTH",
    );
    const emu = build(
      {},
      {
        attributePolicies: {
          ...POLICIES,
          name: { policies: {}, requirements: ["MIN_LENGTH"] },
        } as never,
      },
    );
    expect((await post(emu)).status).toBe(501);
  });
});

describe("G30: a nested object in objectAttributes passes the state-shape check", () => {
  // Round one hides __noSuchMethod__ on plain objects nested in a service's data (AmHostBridge).
  // Discriminating: the failure named a non-enumerable __noSuchMethod__ at objectAttributes.preferences,
  // so the nested object must be a plain object, written via both shared and transient state.
  it.each(["putShared", "putTransient"])(
    "%s of objectAttributes with a nested object",
    async (put) => {
      const emu = make(
        `var oa = { userName: "u", preferences: { updates: true, tags: ["a"], inner: { k: 1 } } };
       nodeState.putShared("username", "u");
       nodeState.${put}("objectAttributes", oa);
       nodeState.putShared("seen", JSON.stringify(nodeState.get("objectAttributes")));
       action.goTo("true");`,
      );
      const result = await start(emu);
      expect(result.status).toBe(200);
      expect(result.body).toMatchObject({ successUrl: expect.any(String) });
    },
  );
  it("a nested object read from existing objectAttributes and written back", async () => {
    const emu = make(
      `nodeState.putShared("username", "u");
       nodeState.putShared("objectAttributes", { preferences: { updates: true } });
       var oa = nodeState.get("objectAttributes");
       oa.preferences.marketing = false;
       nodeState.putShared("objectAttributes", oa);
       action.goTo("true");`,
    );
    expect((await start(emu)).status).toBe(200);
  });
  it("a nested object read from IDM (a service's data) and put into objectAttributes", async () => {
    // Regression guard only: batch 3's G30 named __noSuchMethod__ at objectAttributes.preferences.
    // Commenting out the skip in AmHostBridge's state-shape check does NOT fail this test, so the
    // trigger (which service object carries the hidden member) is not reproduced here.
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
        nodes: {
          ScriptedDecisionNode: {
            one: { script: "main", outcomes: ["true", "false"] },
          },
        },
        scripts: {
          main: decision(
            `var made = openidm.create("managed/widget", null, { name: "n", preferences: { updates: true } });
             var got = openidm.read("managed/widget/" + made._id);
             nodeState.putShared("username", "u");
             nodeState.putTransient("objectAttributes", { name: got.name, preferences: got.preferences });
             action.goTo("true");`,
          ),
        },
      }),
      runner,
      new IdmEmulator(
        new IdmConfig({
          sync: { mappings: [] },
          managed: {
            objects: [
              {
                name: "widget",
                schema: {
                  properties: {
                    name: { type: "string", title: "Name" },
                    preferences: {
                      type: "object",
                      properties: { updates: { type: "boolean", title: "U" } },
                    },
                  },
                },
              },
            ],
          },
        }),
        runner,
      ),
    );
    const result = await start(emu);
    expect(result.status).toBe(200);
  });
});
