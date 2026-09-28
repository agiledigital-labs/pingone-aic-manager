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

  it("returns null for a missing record in a seeded collection", () => {
    const sandbox = loadBehaviour({
      managed: { "managed/alpha_user": [{ _id: "alice" }] },
    });
    const openidm = sandbox.openidm as { read: (id: string) => unknown };
    expect(openidm.read("managed/alpha_user/nobody")).toBeNull();
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

  it("evaluates CREST filters against given.managed with discriminating row sets", () => {
    const rows = [
      {
        _id: "alice",
        userName: "alice",
        mail: "alice@example.com",
        city: "York",
      },
      {
        _id: "alicia",
        userName: "alicia",
        mail: "alicia@other.org",
        city: "York",
      },
      { _id: "bob", userName: "bob", mail: "bob@example.com" },
      {
        _id: "carol",
        userName: "carol",
        mail: "carol@example.com",
        city: "New York",
      },
      { _id: "malice", userName: "malice", mail: "m@x.com" },
    ];
    const sandbox = loadBehaviour({
      managed: { "managed/alpha_user": rows },
    });
    const openidm = sandbox.openidm as {
      query: (
        resource: string,
        params: { _queryFilter: string }
      ) => { result: Array<{ _id: string }> };
    };
    function ids(filter: string): string[] {
      return openidm
        .query("managed/alpha_user", { _queryFilter: filter })
        .result.map((row) => row._id);
    }

    // and binds tighter than or: bob OR (York AND alice-mail) → alice, bob.
    // If or bound tighter, (bob OR York) AND alice-mail → alice only.
    expect(
      ids('userName eq "bob" or city eq "York" and mail eq "alice@example.com"')
    ).toEqual(["alice", "bob"]);

    expect(ids('userName sw "ali"')).toEqual(["alice", "alicia"]);
    expect(ids('userName co "ali"')).toEqual(["alice", "alicia", "malice"]);
    expect(ids("city pr")).toEqual(["alice", "alicia", "carol"]);
    expect(ids('!(userName eq "bob")')).toEqual([
      "alice",
      "alicia",
      "carol",
      "malice",
    ]);
    expect(ids('city eq "New York"')).toEqual(["carol"]);
    expect(ids('/name/last eq "Smith"')).toEqual([]);
  });

  it("matches eq/co against any element of an array-valued field", () => {
    const sandbox = loadBehaviour({
      managed: {
        "managed/alpha_user": [
          { _id: "alice", mail: ["alice@example.com", "a@x.com"] },
          { _id: "bob", mail: ["bob@example.com"] },
        ],
      },
    });
    const openidm = sandbox.openidm as {
      query: (
        resource: string,
        params: { _queryFilter: string }
      ) => { result: Array<{ _id: string }> };
    };
    expect(
      openidm
        .query("managed/alpha_user", {
          _queryFilter: 'mail eq "alice@example.com"',
        })
        .result.map((row) => row._id)
    ).toEqual(["alice"]);
  });

  it("matches a nested JSON-pointer field, not the parent object", () => {
    const sandbox = loadBehaviour({
      managed: {
        "managed/alpha_user": [
          { _id: "alice", name: { first: "Alice", last: "Smith" } },
          { _id: "bob", name: { first: "Bob", last: "Jones" } },
          { _id: "carol", name: { first: "Carol", last: "Smith" } },
        ],
      },
    });
    const openidm = sandbox.openidm as {
      query: (
        resource: string,
        params: { _queryFilter: string }
      ) => { result: Array<{ _id: string }> };
    };
    expect(
      openidm
        .query("managed/alpha_user", { _queryFilter: '/name/last eq "Smith"' })
        .result.map((row) => row._id)
    ).toEqual(["alice", "carol"]);
  });

  it("throws naming an unsupported filter rather than returning empty", () => {
    const sandbox = loadBehaviour({
      managed: {
        "managed/alpha_user": [{ _id: "alice", userName: "alice" }],
      },
    });
    const openidm = sandbox.openidm as {
      query: (resource: string, params: { _queryFilter: string }) => unknown;
    };
    expect(() =>
      openidm.query("managed/alpha_user", { _queryFilter: '/_id ne "alice"' })
    ).toThrow(/unmocked filter "\/_id ne \\"alice\\""/);
    expect(() =>
      openidm.query("managed/alpha_user", {
        _queryFilter: 'not (userName eq "alice")',
      })
    ).toThrow(/unmocked filter/);
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

  it("records the remaining builder types under their Java simple names", () => {
    const effects = runScript(
      [
        'callbacksBuilder.textInputCallback("Email");',
        'callbacksBuilder.scriptTextOutputCallback("js");',
        'callbacksBuilder.pollingWaitCallback("1000", "wait");',
        'callbacksBuilder.redirectCallback("https://x", { a: 1 }, "GET");',
        'callbacksBuilder.validatedUsernameCallback("User", {}, false);',
        'callbacksBuilder.deviceProfileCallback(true, false, "Allow");',
      ].join("\n")
    );
    expect(effects.callbacks.map((cb) => cb.type)).toEqual([
      "TextInputCallback",
      "ScriptTextOutputCallback",
      "PollingWaitCallback",
      "RedirectCallback",
      "ValidatedUsernameCallback",
      "DeviceProfileCallback",
    ]);
  });

  it("covers every remaining builder method once", () => {
    const effects = runScript(
      [
        "callbacksBuilder.suspendedTextOutputCallback(0, 'parked');",
        'callbacksBuilder.languageCallback("en", "GB");',
        'callbacksBuilder.idPCallback("google", "id", "https://r", ["openid"], "n", "req", "https://req", ["acr"], false);',
        'callbacksBuilder.httpCallback("Basic", "Negotiate", "Negotiate", 401);',
        'callbacksBuilder.x509CertificateCallback("cert");',
        'callbacksBuilder.consentMappingCallback({ n: 1 }, "msg", true);',
        'callbacksBuilder.kbaCreateCallback("q", ["a"], false);',
        'callbacksBuilder.selectIdPCallback({ p: true });',
        'callbacksBuilder.termsAndConditionsCallback("1", "terms", "2026-01-01");',
        'callbacksBuilder.metadataCallback({ k: 1 });',
        'callbacksBuilder.stringAttributeInputCallback("mail", "Email", "", true);',
        'callbacksBuilder.numberAttributeInputCallback("age", "Age", 1, true);',
        'callbacksBuilder.booleanAttributeInputCallback("ok", "OK", true, true);',
        'callbacksBuilder.validatedPasswordCallback("pw", false, {}, false);',
      ].join("\n")
    );
    expect(effects.callbacks.map((cb) => cb.type)).toEqual([
      "SuspendedTextOutputCallback",
      "LanguageCallback",
      "IdPCallback",
      "HttpCallback",
      "X509CertificateCallback",
      "ConsentMappingCallback",
      "KbaCreateCallback",
      "SelectIdPCallback",
      "TermsAndConditionsCallback",
      "MetadataCallback",
      "StringAttributeInputCallback",
      "NumberAttributeInputCallback",
      "BooleanAttributeInputCallback",
      "ValidatedPasswordCallback",
    ]);
  });

  it("distinguishes redirectCallback overloads by arity", () => {
    const withCookie = runScript(
      'callbacksBuilder.redirectCallback("https://x", {}, "POST", true);'
    );
    const withStatus = runScript(
      'callbacksBuilder.redirectCallback("https://x", {}, "POST", "status", "cookie");'
    );
    expect(withCookie.callbacks[0]).toEqual({
      type: "RedirectCallback",
      redirectUrl: "https://x",
      redirectData: {},
      method: "POST",
      setTrackingCookie: true,
    });
    expect(withStatus.callbacks[0]).toEqual({
      type: "RedirectCallback",
      redirectUrl: "https://x",
      redirectData: {},
      method: "POST",
      statusParameter: "status",
      redirectBackUrlCookie: "cookie",
    });
  });
});

describe("callbacks (submitted values)", () => {
  it("returns the submitted value itself, in order, not a callback object", () => {
    const sandbox = loadBehaviour({
      callbacks: [
        { type: "NameCallback", value: "alice" },
        { type: "NameCallback", value: "alice.admin" },
        { type: "PasswordCallback", value: "s3cret" },
        { type: "ConfirmationCallback", value: 1 },
      ],
    });
    const callbacks = sandbox.callbacks as {
      isEmpty: () => boolean;
      getNameCallbacks: () => {
        get: (i: number) => unknown;
        size: () => number;
      };
      getPasswordCallbacks: () => { get: (i: number) => unknown };
      getConfirmationCallbacks: () => { get: (i: number) => unknown };
      getChoiceCallbacks: () => { size: () => number };
    };
    expect(callbacks.isEmpty()).toBe(false);
    expect(callbacks.getNameCallbacks().size()).toBe(2);
    expect(callbacks.getNameCallbacks().get(0)).toBe("alice");
    expect(callbacks.getNameCallbacks().get(1)).toBe("alice.admin");
    expect(callbacks.getPasswordCallbacks().get(0)).toBe("s3cret");
    expect(callbacks.getConfirmationCallbacks().get(0)).toBe(1);
    expect(callbacks.getChoiceCallbacks().size()).toBe(0);
  });

  it("treats an explicit empty given.callbacks as a first pass", () => {
    const sandbox = loadBehaviour({ callbacks: [] });
    const callbacks = sandbox.callbacks as { isEmpty: () => boolean };
    expect(callbacks.isEmpty()).toBe(true);
  });

  it("throws naming given.callbacks when the fixture is missing", () => {
    expect(() => runScript("callbacks.getNameCallbacks();")).toThrow(
      /no given\.callbacks/
    );
  });
});
