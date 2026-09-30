import { describe, expect, it } from "vitest";
import { loadBehaviour, runScript } from "./load-behaviour.ts";

describe("require", () => {
  it("is a function on next-gen and undefined on legacy", () => {
    expect(typeof loadBehaviour().require).toBe("function");
    expect(typeof loadBehaviour({ engine: "legacy" }).require).toBe("undefined");
  });

  it("explains why an undeclared top-level require would diverge from AIC", () => {
    expect(() => runScript('require("missing-lib");')).toThrow(
      /library "missing-lib" is not declared on the suite \(given\.libraries\); if this name exists on AIC, AM would load the tenant's copy and the lanes would disagree/
    );
  });

  it("explains a declared library requiring an undeclared tenant library", () => {
    expect(() => runScript('require("declared");', {}, {
      libraries: { declared: 'exports.value = require("tenant-only").value;' },
    })).toThrow(
      /library "tenant-only" is not declared on the suite \(given\.libraries\); if this name exists on AIC, AM would load the tenant's copy and the lanes would disagree/
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

  it("resolves a chain of three libraries", () => {
    const effects = runScript('nodeState.putShared("value", require("chainA").value);', {}, {
      libraries: {
        chainA: 'exports.value = require("chainB").value;',
        chainB: 'exports.value = require("chainC").value;',
        chainC: 'exports.value = "from-c";',
      },
    });
    expect(effects.sharedState.final).toEqual({ value: "from-c" });
  });

  it("runs a shared dependency once per pass", () => {
    const libraries = {
      diamondA: 'exports.value = require("diamondC").value;',
      diamondB: 'exports.value = require("diamondC").value;',
      diamondC: [
        'var count = Number(nodeState.get("cLoads") || 0) + 1;',
        'nodeState.putShared("cLoads", count);',
        'exports.value = "from-c";',
      ].join("\n"),
    };
    const script = [
      'nodeState.putShared("values", require("diamondA").value + ":" + require("diamondB").value);',
      'nodeState.putShared("observedLoads", nodeState.get("cLoads"));',
    ].join("\n");
    for (let pass = 0; pass < 2; pass += 1) {
      const effects = runScript(script, {}, { libraries });
      expect(effects.sharedState.final).toEqual({
        cLoads: 1, values: "from-c:from-c", observedLoads: 1,
      });
    }
  });

  it("returns partially built exports to the second library in a mutual require", () => {
    const effects = runScript([
      'var a = require("cycleA");',
      'nodeState.putShared("value", a.fromB);',
    ].join("\n"), {}, {
      libraries: {
        cycleA: 'exports.name = "a"; exports.fromB = require("cycleB").sawA;',
        cycleB: 'exports.sawA = require("cycleA").name;',
      },
    });
    expect(effects.sharedState.final).toEqual({ value: "a" });
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
