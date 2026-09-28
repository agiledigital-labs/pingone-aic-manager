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

describe("mergeChannels", () => {
  it("merges per key, so a test override keeps the suite's other keys", () => {
    const draft = mergeChannels(
      { headers: { "accept-language": "en-AU", "x-a": "1" } },
      { headers: { "x-a": "2" } }
    );
    expect(draft.headers).toEqual({ "accept-language": ["en-AU"], "x-a": ["2"] });
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
      headers: {},
      params: {},
      session: {},
      sessionRequested: false,
    });
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
  it("lands inputs under their own names and ESVs under the esv prefix", () => {
    const draft = mergeChannels({ esv: { "idr.threshold": "0.8" } }, undefined);
    applyInputsAndEsv(draft, { userId: "alice", debug: false });
    expect(draft.state.shared).toEqual({
      userId: "alice",
      debug: false,
      "esv.idr.threshold": "0.8",
    });
  });

  it("refuses an input that would collide with the esv namespace", () => {
    const draft = mergeChannels(undefined, undefined);
    expect(() => applyInputsAndEsv(draft, { "esv.x": "1" })).toThrow(
      /reserved for ESV overrides/
    );
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
  it("omits channels nothing was put in", () => {
    const given = toGiven(mergeChannels(undefined, undefined));
    expect(given).toEqual({});
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
