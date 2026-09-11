import { describe, expect, it } from "vitest";
import { loadBehaviour, runScript } from "./load-behaviour.ts";

describe("logger", () => {
  it("formats slf4j {} placeholders, escapes, and surplus args", () => {
    const effects = runScript(
      [
        'logger.error("a");',
        'logger.error("a {} b", "X");',
        'logger.error("a {} b {} c", "X", "Y");',
        'logger.error("a {} b {} c", "X");',
        'logger.error("a", "X");',
        'logger.error("a \\\\{} b", "X");',
        'logger.error("a \\\\\\\\{} b", "X");',
        'logger.info("plain");',
        'logger.warn("two {} and {}", 1, true);',
        'logger.debug("one {} bound", "alice");',
      ].join("\n")
    );
    expect(effects.logs.map((line) => `${line.level}:${line.message}`)).toEqual([
      "error:a",
      "error:a X b",
      "error:a X b Y c",
      "error:a X b {} c",
      "error:a",
      "error:a {} b",
      "error:a \\X b",
      "info:plain",
      "warn:two 1 and true",
      "debug:one alice bound",
    ]);
  });

  it("does not treat a JS Error as a throwable", () => {
    const effects = runScript('logger.error("a", new Error("boom"));');
    expect(effects.logs).toEqual([{ level: "error", message: "a" }]);
  });
});

describe("openidm", () => {
  it("reads a seeded record and records the effect without a body", () => {
    const effects = runScript('openidm.read("managed/alpha_user/alice");', {
      managed: {
        "managed/alpha_user": [{ _id: "alice", mail: "alice@example.com" }],
      },
    });
    expect(effects.openidm).toEqual([
      { method: "read", resource: "managed/alpha_user/alice" },
    ]);
  });

  it("throws naming the missing given.managed entry", () => {
    expect(() => runScript('openidm.read("managed/alpha_user/alice");')).toThrow(
      /no given\.managed entry for "managed\/alpha_user"/
    );
  });

  it("records create with a null id as the collection path", () => {
    const effects = runScript(
      'openidm.create("managed/alpha_user", null, { userName: "bob" });',
      { managed: { "managed/alpha_user": [] } }
    );
    expect(effects.openidm).toEqual([
      {
        method: "create",
        resource: "managed/alpha_user",
        body: { userName: "bob" },
      },
    ]);
  });

  it("records patch body as the patch array and drops rev", () => {
    const effects = runScript(
      'openidm.patch("managed/alpha_user/alice", "1", [{ operation: "replace", field: "sn", value: "smith" }]);',
      {
        managed: {
          "managed/alpha_user": [{ _id: "alice", sn: "a", _rev: "1" }],
        },
      }
    );
    expect(effects.openidm).toEqual([
      {
        method: "patch",
        resource: "managed/alpha_user/alice",
        body: [{ operation: "replace", field: "sn", value: "smith" }],
      },
    ]);
  });

  it("records query params as body", () => {
    const effects = runScript(
      'openidm.query("managed/alpha_user", { _queryFilter: "true" });',
      {
        managed: {
          "managed/alpha_user": [{ _id: "alice", userName: "alice" }],
        },
      }
    );
    expect(effects.openidm).toEqual([
      {
        method: "query",
        resource: "managed/alpha_user",
        body: { _queryFilter: "true" },
      },
    ]);
  });

  it("records actionName and body as content ?? params", () => {
    const withContent = runScript(
      'openidm.action("managed/alpha_user/alice", "reset", { n: 1 }, { q: true });'
    );
    expect(withContent.openidm).toEqual([
      {
        method: "action",
        resource: "managed/alpha_user/alice",
        body: { n: 1 },
        actionName: "reset",
      },
    ]);
    const paramsOnly = runScript(
      'openidm.action("managed/alpha_user/alice", "reset", null, { q: true });'
    );
    expect(paramsOnly.openidm).toEqual([
      {
        method: "action",
        resource: "managed/alpha_user/alice",
        body: { q: true },
        actionName: "reset",
      },
    ]);
  });
});

describe("httpClient", () => {
  it("replies from the first matching given.http stub", () => {
    const sandbox = loadBehaviour({
      http: [
        {
          match: { url: "https://example.com/x", method: "POST" },
          reply: { status: 201, body: { ok: true } },
        },
      ],
    });
    const httpClient = sandbox.httpClient as {
      send: (
        url: string,
        opts: { method: string; body: unknown }
      ) => { get: () => { status: number; ok: boolean; json: () => unknown } };
    };
    const response = httpClient
      .send("https://example.com/x", { method: "POST", body: { a: 1 } })
      .get();
    expect(response.status).toBe(201);
    expect(response.ok).toBe(true);
    expect(response.json()).toEqual({ ok: true });
  });

  it("throws naming the URL when no stub matches", () => {
    expect(() =>
      runScript('httpClient.send("https://example.com/missing").get();')
    ).toThrow(/no given\.http stub for GET https:\/\/example.com\/missing/);
  });

  it("records the request on the http channel", () => {
    const effects = runScript(
      'httpClient.send("https://example.com/x", { method: "GET" }).get();',
      {
        http: [{ match: { url: "https://example.com/x" }, reply: { status: 200 } }],
      }
    );
    expect(effects.http).toEqual([{ url: "https://example.com/x", method: "GET" }]);
  });
});

describe("callbacksBuilder", () => {
  it("records Java simple class names as callback type", () => {
    const effects = runScript(
      [
        'callbacksBuilder.nameCallback("User Name");',
        'callbacksBuilder.passwordCallback("Password", false);',
        'callbacksBuilder.textOutputCallback(0, "hello");',
        'callbacksBuilder.hiddenValueCallback("id", "v");',
        'callbacksBuilder.confirmationCallback(0, ["Yes", "No"], 0);',
        'callbacksBuilder.choiceCallback("Pick", ["a", "b"], 0, false);',
      ].join("\n")
    );
    expect(effects.callbacks.map((cb) => cb.type)).toEqual([
      "NameCallback",
      "PasswordCallback",
      "TextOutputCallback",
      "HiddenValueCallback",
      "ConfirmationCallback",
      "ChoiceCallback",
    ]);
  });

  it("leaves unimplemented callback builders throwing", () => {
    const sandbox = loadBehaviour();
    const builder = sandbox.callbacksBuilder as {
      redirectCallback: (url: string, data: object, method: string) => void;
    };
    expect(() => builder.redirectCallback("https://x", {}, "GET")).toThrow(
      /not mocked: callbacksBuilder\.redirectCallback/
    );
  });
});
