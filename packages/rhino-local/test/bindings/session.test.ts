import { describe, expect, it } from "vitest";
import type { Given } from "../../src/case/types.ts";
import { loadBehaviour, runScript } from "./load-behaviour.ts";

/**
 * Every assertion here names something measured on a live tenant 2026-09-14
 * (docs/api/12-script-bindings-matrix.md). The shape is a Java map behind
 * AM's `MapScriptWrapper`, not a plain object, and the differences run in the
 * false-pass direction — a plain-object mock would let `hasOwnProperty` and
 * `keySet` through here and throw there.
 */

function keysOf(value: object): string[] {
  const keys: string[] = [];
  for (const key in value) {
    keys.push(key);
  }
  return keys;
}

const SEED = {
  UserId: "alice",
  Principals: "alice",
  AuthLevel: "0",
  rlProbe: "hello",
};

type SessionMap = Record<string, string> & {
  get: (key: string) => string | null;
  put: (key: string, value: string) => void;
  containsKey: (key: string) => boolean;
  size: () => number;
  isEmpty: () => boolean;
  keySet: () => unknown;
};

function session(given: Given = { existingSession: SEED }): SessionMap {
  return loadBehaviour(given).existingSession as SessionMap;
}

describe("existingSession", () => {
  it("is undefined when the request carries no session cookie", () => {
    const sandbox = loadBehaviour({});
    expect(sandbox.existingSession).toBeUndefined();
    expect(runScript('logger.info(typeof existingSession); action.goTo("true");').logs).toEqual(
      [{ level: "info", message: "undefined" }]
    );
  });

  it("reads as a flat string map by key, dot, get() and for...in", () => {
    const map = session();
    expect(map.UserId).toBe("alice");
    expect(map["sun.am.UniversalIdentifier"]).toBeUndefined();
    expect(map.get("UserId")).toBe("alice");
    expect(map.get("nope")).toBeNull();
    expect(map.containsKey("rlProbe")).toBe(true);
    expect(map.containsKey("nope")).toBe(false);
    expect(map.size()).toBe(4);
    expect(map.isEmpty()).toBe(false);
    expect(keysOf(map).sort()).toEqual(["AuthLevel", "Principals", "UserId", "rlProbe"]);
    expect("UserId" in map).toBe(true);
  });

  it("coerces every value to a string, as AM stores them", () => {
    // `AuthLevel` is the live example: AM reports the number 0 as "0".
    const map = session({ existingSession: { AuthLevel: 0 as unknown as string } });
    expect(map.AuthLevel).toBe("0");
    expect(typeof map.get("AuthLevel")).toBe("string");
  });

  it("does not expose hasOwnProperty, which AM's wrapper lacks", () => {
    // The discriminating case: a plain-object mock passes this call and the
    // tenant throws, which is the one failure a two-lane harness cannot see.
    const map = session();
    expect((map as unknown as { hasOwnProperty?: unknown }).hasOwnProperty).toBeUndefined();
    expect(Object.prototype.hasOwnProperty.call(map, "UserId")).toBe(true);
  });

  it("throws from keySet(), which AM's class shutter refuses", () => {
    expect(() => session().keySet()).toThrow(/class shutter/);
  });

  it("renders toString per evaluator, because the wrapper differs", () => {
    expect(String(session())).toBe(
      '{ "UserId": "alice", "Principals": "alice", "AuthLevel": "0", "rlProbe": "hello" }'
    );
    expect(
      String(session({ existingSession: SEED, engine: "legacy" }))
    ).toBe("{UserId=alice, Principals=alice, AuthLevel=0, rlProbe=hello}");
  });

  it("enforces get()'s arity, as the live InternalError does", () => {
    const map = session();
    expect(() => (map.get as (...args: unknown[]) => unknown)()).toThrow(/arity=0/);
  });

  it("is reinstalled per case, so one case's session cannot leak into the next", () => {
    expect(loadBehaviour({ existingSession: SEED }).existingSession).toBeDefined();
    expect(loadBehaviour({}).existingSession).toBeUndefined();
  });

  it("survives a write, matching the measured put/assign behaviour", () => {
    const map = session();
    map.put("extra", "1");
    expect(map.get("extra")).toBe("1");
    expect(map.size()).toBe(5);
  });
});
