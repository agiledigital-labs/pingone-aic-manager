import { describe, expect, it } from "vitest";
import { runScript } from "./load-behaviour.ts";

// Both Java-map surfaces share one model, so both are driven through it.
const SOURCES = [
  ["registered objectAttributes", 'var map = nodeState.get("objectAttributes");', { registeredObjectAttributes: { seeded: "s", get: "seeded-get" } }],
  ["JsonValue.object()", 'var map = org.forgerock.json.JsonValue.object(); map.put("seeded", "s"); map.put("get", "seeded-get");', {}],
] as const;

describe.each(SOURCES)("%s keys named like its methods", (_name, open, given) => {
  function probe(lines: string[]) {
    const effects = runScript(
      [
        'function t(f) { try { return String(f()); } catch (e) { return "threw:" + e; } }',
        open,
        ...lines,
      ].join("\n"),
      { ...given }
    );
    return effects.sharedState.final;
  }

  it("treats a missing method-named key as absent", () => {
    expect(probe([
      'nodeState.putShared("hasPut", t(function () { return map.containsKey("put"); }));',
      'nodeState.putShared("getSize", t(function () { return map.get("size"); }));',
    ])).toMatchObject({ hasPut: "false", getSize: "null" });
  });

  it("reads a seeded method-named key without losing the method", () => {
    expect(probe([
      'nodeState.putShared("hasGet", t(function () { return map.containsKey("get"); }));',
      'nodeState.putShared("getGet", t(function () { return map.get("get"); }));',
      'nodeState.putShared("size", t(function () { return map.size(); }));',
    ])).toMatchObject({ hasGet: "true", getGet: "seeded-get", size: "2" });
  });

  it("inserts method-named keys, keeps the methods, and harvests every entry", () => {
    const final = probe([
      'nodeState.putShared("putPrevious", t(function () { return map.put("put", "p"); }));',
      'map.put("size", "z");',
      'map.put("after", "a");',
      'nodeState.putShared("afterGet", t(function () { return map.get("put") + "," + map.get("size") + "," + map.get("seeded"); }));',
      'nodeState.putShared("afterSize", t(function () { return map.size(); }));',
      'nodeState.putShared("printed", String(map));',
      'nodeState.putShared("copy", JSON.parse(JSON.stringify(map)));',
    ]);
    expect(final).toMatchObject({
      putPrevious: "null",
      afterGet: "p,z,s",
      afterSize: "5",
      printed: '{ "seeded": "s", "get": "seeded-get", "put": "p", "size": "z", "after": "a" }',
      copy: { seeded: "s", get: "seeded-get", put: "p", size: "z", after: "a" },
    });
  });
});

describe("registered objectAttributes harvest", () => {
  it("keeps a method-named entry in the recorded state", () => {
    const effects = runScript('nodeState.get("objectAttributes").put("get", "v");', {
      registeredObjectAttributes: { seeded: "s" },
    });
    expect(effects.sharedState.final.objectAttributes).toEqual({ seeded: "s", get: "v" });
  });
});
