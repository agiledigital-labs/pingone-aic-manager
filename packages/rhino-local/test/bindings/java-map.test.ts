import { describe, expect, it } from "vitest";
import { runScript } from "./load-behaviour.ts";

// Both Java-map surfaces share one model, so both are driven through it.
// Expected values are the ones measured on AIC (live-java-map-enumeration).
const SOURCES = [
  ["registered objectAttributes", 'var map = nodeState.get("objectAttributes");', { registeredObjectAttributes: { plain: "p" } }],
  ["JsonValue.object()", 'var map = org.forgerock.json.JsonValue.object(); map.put("plain", "p");', {}],
] as const;

const shutter = (javaClass: string) =>
  `threw:InternalError: Access to Java class "${javaClass}" is prohibited.`;

describe.each(SOURCES)("%s as a Java map", (_name, open, given) => {
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

  it("keeps its methods while no entry is named like one", () => {
    expect(probe([
      'nodeState.putShared("getMissing", t(function () { return map.get("size"); }));',
      'nodeState.putShared("hasPut", t(function () { return map.containsKey("put"); }));',
      'nodeState.putShared("size", t(function () { return map.size(); }));',
      'nodeState.putShared("putNew", t(function () { return map.put("fresh", "f"); }));',
      'nodeState.putShared("putExisting", t(function () { return map.put("plain", "p2"); }));',
    ])).toMatchObject({ getMissing: "null", hasPut: "false", size: "1", putNew: "null", putExisting: "p" });
  });

  // The discriminating case: an entry named like a method replaces it.
  it("lets an entry named like a method shadow it, and enumerates it", () => {
    expect(probe([
      'map.put("get", "g");',
      'nodeState.putShared("dot", t(function () { return typeof map.get; }));',
      'nodeState.putShared("index", t(function () { return map["get"]; }));',
      'nodeState.putShared("call", t(function () { try { map.get("plain"); return "ok"; } catch (e) { return e.name; } }));',
      'nodeState.putShared("contains", t(function () { return map.containsKey("get"); }));',
      'nodeState.putShared("forIn", t(function () { var out = []; for (var k in map) { out.push(k); } return out.sort().join(","); }));',
      'nodeState.putShared("keys", t(function () { return Object.keys(map).sort().join(","); }));',
      'nodeState.putShared("printed", String(map));',
    ])).toMatchObject({
      dot: "string",
      index: "g",
      call: "TypeError",
      contains: "true",
      forIn: "get,plain",
      keys: "get,plain",
      printed: '{ "plain": "p", "get": "g" }',
    });
  });

  it("refuses keySet and the entries of entrySet, and snapshots both views", () => {
    expect(probe([
      'map.put("other", "o");',
      'nodeState.putShared("keySet", t(function () { map.keySet(); return "ok"; }));',
      'nodeState.putShared("entrySize", t(function () { var es = map.entrySet(); map.put("later", "l"); return es.size(); }));',
      'nodeState.putShared("entryHasNext", t(function () { return map.entrySet().iterator().hasNext(); }));',
      'nodeState.putShared("entryNext", t(function () { return map.entrySet().iterator().next(); }));',
      'nodeState.putShared("entryLength", t(function () { return map.entrySet().toArray().length; }));',
      'nodeState.putShared("entryElement", t(function () { return map.entrySet().toArray()[0]; }));',
      'nodeState.putShared("valuesSize", t(function () { var vs = map.values(); map.put("later2", "l2"); return vs.size(); }));',
      'nodeState.putShared("valuesGet", t(function () { return map.values().get(0); }));',
      'nodeState.putShared("valuesArray", t(function () { return Array.prototype.slice.call(map.values().toArray()).join(","); }));',
      'nodeState.putShared("valuesIter", t(function () { return map.values().iterator(); }));',
    ])).toMatchObject({
      keySet: shutter("java.util.LinkedHashMap$LinkedKeySet"),
      entrySize: "2",
      entryHasNext: "true",
      entryNext: shutter("java.util.LinkedHashMap$Entry"),
      entryLength: "3",
      entryElement: shutter("java.util.LinkedHashMap$Entry"),
      valuesSize: "3",
      valuesGet: "p",
      valuesArray: "p,o,l,l2",
      valuesIter: shutter("java.util.ArrayList$Itr"),
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
