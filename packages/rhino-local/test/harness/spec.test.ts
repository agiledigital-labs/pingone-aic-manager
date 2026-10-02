import { describe, expect, it } from "vitest";
import { DEFAULT_SESSION_PRINCIPAL } from "../../src/case/session.ts";
import { z } from "zod";
import {
  applyInputsAndEsv,
  mergeChannels,
  normaliseWire,
  parseInputs,
  toCase,
  toGiven,
} from "../../src/harness/spec.ts";
import type { RequestDraft } from "../../src/harness/index.ts";

describe("mergeChannels", () => {
  it("merges per key, so a test override keeps the suite's other keys", () => {
    const draft = mergeChannels(
      { headers: { "accept-language": "en-AU", "x-a": "1" } },
      { headers: { "x-a": "2" } }
    );
    expect(draft.headers).toEqual({ "accept-language": ["en-AU"], "x-a": ["2"] });
  });

  it("merges registered object attributes through suite, test, and beforeRun", () => {
    const draft = mergeChannels(
      { registeredObjectAttributes: { suite: "one", same: "old" } },
      { registeredObjectAttributes: { same: "new" } }
    );
    if (draft.registeredObjectAttributes === undefined) throw new Error("missing registered map");
    draft.registeredObjectAttributes.hook = "three";
    expect(toGiven(draft).registeredObjectAttributes).toEqual({ suite: "one", same: "new", hook: "three" });
    expect(() => toGiven(mergeChannels(
      { state: { shared: { objectAttributes: {} } }, registeredObjectAttributes: {} }, undefined
    ))).toThrow(/collides with state.shared.objectAttributes/);
  });

  // The discriminating case. A channel-level merge — `{...always, ...override}`
  // one level up — passes any test that only overrides, and silently drops
  // `accept-language` here. That is the shape that makes `always` useless and
  // sends people back to copying defaults into every test.
  it("does not replace a whole channel when one key is overridden", () => {
    const draft = mergeChannels(
      { state: { shared: { realmName: "alpha", tier: "gold" } } },
      { state: { shared: { tier: "silver" } } }
    );
    expect(draft.state.shared).toEqual({ realmName: "alpha", tier: "silver" });
  });

  it("starts every channel present and empty", () => {
    const draft = mergeChannels(undefined, undefined);
    expect(draft).toEqual({
      state: { shared: {}, transient: {} },
      esv: {},
      esvInState: false,
      esvUndeclared: "error",
      headers: {},
      params: {},
      cookies: {},
      http: [],
      openidmActions: [],
      openidmFailures: [],
      bindingOverrides: {},
      identityAttributes: {},
      session: {},
      sessionRequested: false,
    });
  });

  it("merges request cookies per name and emits them in Given", () => {
    const draft = mergeChannels({ cookies: { suite: "one", same: "old" } }, { cookies: { same: "new" } });
    draft.cookies.hook = "three";
    expect(toGiven(draft).requestCookies).toEqual({ suite: "one", same: "new", hook: "three" });
  });

  it("places per-test HTTP stubs before suite defaults", () => {
    const suiteStub = { match: { url: "https://example.com/api" }, reply: { status: 200 } };
    const testStub = { match: { url: "https://example.com/api" }, reply: { status: 503 } };
    const draft = mergeChannels({ http: [suiteStub] }, { http: [testStub] });
    expect(toGiven(draft).http).toEqual([testStub, suiteStub]);
  });

  it("merges binding replacements by name and keeps beforeRun changes", () => {
    const draft = mergeChannels(
      { bindingOverrides: { logger: "({getName: function(){return 'suite';}})", idRepository: "({})" } },
      { bindingOverrides: { logger: "({getName: function(){return 'test';}})" } }
    );
    draft.bindingOverrides.idRepository = "({getIdentity: function(){return 'hook';}})";
    expect(toGiven(draft).bindingOverrides).toEqual(draft.bindingOverrides);
  });

  it("merges declared identity attributes per attribute and carries them into given", () => {
    const draft = mergeChannels(
      { identityAttributes: { a: { field: "suiteA", cardinality: "single" }, b: { field: "suiteB", cardinality: "multi" } } },
      { identityAttributes: { a: { field: "testA", cardinality: "multi" } } }
    );
    expect(draft.identityAttributes).toEqual({
      a: { field: "testA", cardinality: "multi" },
      b: { field: "suiteB", cardinality: "multi" },
    });
    expect(toGiven(draft).identityAttributes).toEqual(draft.identityAttributes);
    expect(toGiven(mergeChannels(undefined, undefined)).identityAttributes).toBeUndefined();
  });

  // A beforeRun hook mutates the draft in place. A nested entry shared with
  // the suite or the override would carry that edit into every later run.
  it.each([
    ["identityAttributes", (d: RequestDraft) => { d.identityAttributes!.displayName!.field = "description"; }],
    ["http", (d: RequestDraft) => { d.http![0]!.reply.status = 500; }],
    ["openidmFailures", (d: RequestDraft) => { d.openidmFailures![0]!.reply.code = 500; }],
    ["state", (d: RequestDraft) => { (d.state.shared.nested as Record<string, unknown>).k = "hook"; }],
    ["registeredObjectAttributes", (d: RequestDraft) => { (d.registeredObjectAttributes!.nested as Record<string, unknown>).k = "hook"; }],
    ["session", (d: RequestDraft) => { (d.session.nested as Record<string, unknown>).k = "hook"; }],
  ] as const)("isolates %s entries from the suite and from the next run", (_channel, mutate) => {
    const channels = () => ({
      identityAttributes: { displayName: { field: "displayName", cardinality: "single" as const } },
      http: [{ match: { url: "https://example.com/api" }, reply: { status: 200 } }],
      openidmFailures: [{ match: { method: "patch" as const, resource: "managed/alpha_user", ordinal: 1 }, reply: { code: 409 } }],
      state: { shared: { nested: { k: "suite" } } },
      registeredObjectAttributes: { nested: { k: "suite" } },
      session: { nested: { k: "suite" } },
    });
    const always = channels();
    const override = { session: { nested: { k: "suite" } } };
    mutate(mergeChannels(always, override));
    expect(always).toEqual(channels());
    expect(override).toEqual({ session: { nested: { k: "suite" } } });
    expect(mergeChannels(always, override)).toEqual(mergeChannels(channels(), { session: { nested: { k: "suite" } } }));
  });
});

describe("normaliseWire", () => {
  it("treats a bare string as a one-element list", () => {
    expect(normaliseWire({ a: "x" })).toEqual({ a: ["x"] });
  });

  it("keeps an array as repeated occurrences in send order", () => {
    expect(normaliseWire({ xff: ["203.0.113.1", "198.51.100.7"] })).toEqual({
      xff: ["203.0.113.1", "198.51.100.7"],
    });
  });

  it("rejects an empty array rather than sending a valueless key", () => {
    expect(() => normaliseWire({ a: [] })).toThrow(/empty array/);
  });
});

describe("applyInputsAndEsv", () => {
  it("lands inputs in state and ESVs in the systemEnv binding", () => {
    const draft = mergeChannels({ esv: { "idr.threshold": "0.8" } }, undefined);
    applyInputsAndEsv(draft, { userId: "alice", debug: false });
    expect(draft.state.shared).toEqual({
      userId: "alice",
      debug: false,
    });
    expect(toGiven(draft).esv).toEqual({ "esv.idr.threshold": "0.8" });
  });

  it("mirrors ESVs into state only when requested and rejects collisions then", () => {
    const draft = mergeChannels({ esv: { x: "1" }, esvInState: true }, undefined);
    applyInputsAndEsv(draft, { userId: "alice" });
    expect(draft.state.shared["esv.x"]).toBe("1");
    expect(() => applyInputsAndEsv(draft, { "esv.x": "1" })).toThrow(
      /reserved for ESV overrides/
    );
  });

  it("allows an esv-prefixed input when the mirror is off", () => {
    const draft = mergeChannels(undefined, undefined);
    applyInputsAndEsv(draft, { "esv.x": "input" });
    expect(draft.state.shared["esv.x"]).toBe("input");
  });

  // Regression: full property names used to become silently double-prefixed.
  it("rejects full property names and management API ids", () => {
    const property = mergeChannels({ esv: { "esv.x": "1" }, esvInState: true }, undefined);
    expect(() => applyInputsAndEsv(property, {})).toThrow(/use "x" for systemEnv/);
    expect(property.state.shared).toEqual({});
    expect(() => toGiven(property)).toThrow(/already includes "esv\."/);

    const managementId = mergeChannels({ esv: { "esv-x": "1" } }, undefined);
    expect(() => applyInputsAndEsv(managementId, {})).toThrow(/management API id/);
    expect(() => toGiven(managementId)).toThrow(/management API id/);

    const mutated = mergeChannels(undefined, undefined);
    mutated.esv["esv.x"] = "1";
    expect(() => applyInputsAndEsv(mutated, {})).toThrow(/already includes "esv\."/);
  });
});

describe("parseInputs", () => {
  const inputs = z.object({
    userId: z.string(),
    locale: z.enum(["en-US", "en-AU"]).default("en-AU"),
    debug: z.boolean().default(false),
  });

  it("supplies declared defaults for optional inputs", () => {
    expect(parseInputs({ name: "s", inputs }, { userId: "alice" })).toEqual({
      userId: "alice",
      locale: "en-AU",
      debug: false,
    });
  });

  it("names the missing required input", () => {
    expect(() => parseInputs({ name: "s", inputs }, {})).toThrow(/userId/);
  });

  it("rejects a value outside a declared union", () => {
    expect(() =>
      parseInputs({ name: "s", inputs }, { userId: "a", locale: "en-NZ" })
    ).toThrow(/locale/);
  });

  it("rejects inputs when the suite declares none", () => {
    expect(() => parseInputs({ name: "s" }, { userId: "a" })).toThrow(
      /declares none/
    );
  });
});

describe("toGiven", () => {
  it("seeds an empty ESV declaration for fail-closed reads", () => {
    const given = toGiven(mergeChannels(undefined, undefined));
    expect(given).toEqual({ esv: {}, esvUndeclared: "error" });
  });

  it("compiles the session channel to existingSession, merged per key", () => {
    const draft = mergeChannels(
      { session: { tier: "gold", step: "suite" } },
      { session: { step: "test" } }
    );
    expect(toGiven(draft).existingSession).toMatchObject({
      tier: "gold",
      step: "test",
    });
  });

  it("mints the AM-derived properties for a session declared empty", () => {
    // `session: {}` is a request for a logged-in session with no extra
    // properties — distinct from not asking for one at all.
    const given = toGiven(
      mergeChannels({ state: { shared: { username: "alice" } }, session: {} }, undefined)
    );
    expect(given.existingSession).toEqual({
      UserId: "alice",
      Principals: "alice",
      UserToken: "alice",
      Principal: "id=alice,ou=user,o=alpha,ou=services,ou=am-config",
      "sun.am.UniversalIdentifier":
        "id=alice,ou=user,o=alpha,ou=services,ou=am-config",
    });
  });

  it("falls back to a fixed principal when no username is in state", () => {
    const given = toGiven(mergeChannels({ session: { tier: "gold" } }, undefined));
    expect(given.existingSession?.UserId).toBe(DEFAULT_SESSION_PRINCIPAL);
    expect(given.existingSession?.tier).toBe("gold");
  });

  it("refuses an AM-owned key, naming the principal mechanism", () => {
    // Measured: overriding one fails the whole login with a bare 401, so the
    // alternative to refusing is an unexplained authentication failure.
    expect(() => toGiven(mergeChannels({ session: { UserId: "bob" } }, undefined)))
      .toThrow(/set by AM.*state\.username/s);
    expect(() => toGiven(mergeChannels({ session: { AuthLevel: "9" } }, undefined)))
      .toThrow(/set by AM/);
  });

  it("leaves existingSession absent when no session is declared", () => {
    // Absent and empty are different bindings: AM installs nothing at all
    // without a session cookie, so `typeof existingSession` is "undefined".
    expect(toGiven(mergeChannels({ state: { shared: { a: 1 } } }, undefined)).existingSession)
      .toBeUndefined();
  });

  it("coerces session values to strings and refuses structured ones", () => {
    expect(toGiven(mergeChannels({ session: { attempts: 0 } }, undefined)).existingSession)
      .toMatchObject({ attempts: "0" });
    expect(() => toGiven(mergeChannels({ session: { u: { id: "a" } } }, undefined)))
      .toThrow(/session\.u must be a string/);
    expect(() => toGiven(mergeChannels({ session: { u: null } }, undefined)))
      .toThrow(/session\.u must be a string/);
  });
});

describe("toCase", () => {
  it("carries the suite's outcomes onto the case both lanes judge", () => {
    const draft = mergeChannels({ state: { shared: { a: 1 } } }, undefined);
    const kase = toCase(
      { name: "resolve-identity", script: "src", outcomes: ["matched", "notFound"] },
      "resolve-identity > matches one",
      draft,
      { outcome: "matched" }
    );
    expect(kase.name).toBe("resolve-identity > matches one");
    expect(kase.outcomes).toEqual(["matched", "notFound"]);
    expect(kase.given.sharedState).toEqual({ a: 1 });
  });
});

describe("a request draft written against 0.1.2", () => {
  // The 0.1.2 RequestDraft had none of cookies, http, openidmFailures,
  // bindingOverrides or esvInState. Typed as the public RequestDraft, this
  // literal is also the compile-time check that it still type-checks.
  function draft012(): RequestDraft {
    return {
      state: { shared: { username: "alice" }, transient: {} },
      esv: { probe: "value" },
      headers: { "x-probe": ["1"] },
      params: {},
      session: {},
      sessionRequested: false,
    };
  }

  it("turns into a Given with the new channels empty", () => {
    const given = toGiven(draft012());
    expect(given.sharedState).toEqual({ username: "alice" });
    expect(given.esv).toEqual({ "esv.probe": "value" });
    expect(given.requestHeaders).toEqual({ "x-probe": ["1"] });
    expect(given.requestCookies).toBeUndefined();
    expect(given.http).toBeUndefined();
    expect(given.openidmFailures).toBeUndefined();
    expect(given.bindingOverrides).toBeUndefined();
  });

  it("goes through applyInputsAndEsv and toCase", () => {
    const draft = draft012();
    applyInputsAndEsv(draft, { extra: "x" });
    const kase = toCase({ name: "compat", script: "", outcomes: ["true"] }, "compat", draft, { outcome: "true" });
    expect(kase.given.sharedState).toEqual({ username: "alice", extra: "x" });
  });
});
