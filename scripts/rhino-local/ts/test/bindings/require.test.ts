import { describe, expect, it } from "vitest";
import { loadBehaviour, runScript } from "./load-behaviour.ts";

describe("require", () => {
  it("is a function on next-gen and undefined on legacy", () => {
    expect(typeof loadBehaviour().require).toBe("function");
    expect(typeof loadBehaviour({ engine: "legacy" }).require).toBe("undefined");
  });

  it("throws naming a missing given.libraries entry", () => {
    expect(() => runScript('require("missing-lib");')).toThrow(
      /no given\.libraries entry for "missing-lib"/
    );
  });

  it("returns exports from a seeded library, including top-level const", () => {
    const sandbox = loadBehaviour(
      {},
      {
        libraries: {
          "rhino-lib-const-probe": [
            'const TOP_CONST = "lib-const-ok";',
            'var TOP_VAR = "lib-var-ok";',
            "exports.fromConst = TOP_CONST;",
            "exports.fromVar = TOP_VAR;",
          ].join("\n"),
        },
      }
    );
    const requireFn = sandbox.require as (name: string) => {
      fromConst: string;
      fromVar: string;
    };
    expect(requireFn("rhino-lib-const-probe")).toEqual({
      fromConst: "lib-const-ok",
      fromVar: "lib-var-ok",
    });
  });

  it("lets a library call openidm.read against given.managed", () => {
    const sandbox = loadBehaviour(
      {
        managed: {
          "managed/alpha_name_variant": [
            { _id: "aaron_erin", nameA: "alice", nameB: "bob" },
          ],
        },
      },
      {
        libraries: {
          "rhino-lib-openidm-read-probe": [
            'exports.rec = openidm.read("managed/alpha_name_variant/aaron_erin");',
          ].join("\n"),
        },
      }
    );
    const requireFn = sandbox.require as (name: string) => {
      rec: { nameA: string; nameB: string };
    };
    expect(requireFn("rhino-lib-openidm-read-probe").rec.nameA).toBe("alice");
  });

  it("resolves a nested require against the same given.libraries map", () => {
    const sandbox = loadBehaviour(
      {},
      {
        libraries: {
          inner: "exports.stamp = 'from-inner';",
          outer: 'exports.stamp = require("inner").stamp;',
        },
      }
    );
    const requireFn = sandbox.require as (name: string) => { stamp: string };
    expect(requireFn("outer").stamp).toBe("from-inner");
  });

  it("rejects the wrong arity", () => {
    const requireFn = loadBehaviour().require as (...args: unknown[]) => unknown;
    expect(() => requireFn()).toThrow(/require arity=0 \(expected 1\)/);
  });
});

describe("require cache", () => {
  it("returns the same exports object on a second require of the same id", () => {
    const sandbox = loadBehaviour(
      {},
      { libraries: { once: "exports.n = (exports.n || 0) + 1;" } }
    );
    const requireFn = sandbox.require as (name: string) => { n: number };
    const first = requireFn("once");
    const second = requireFn("once");
    expect(first).toBe(second);
    expect(first.n).toBe(1);
  });
});


