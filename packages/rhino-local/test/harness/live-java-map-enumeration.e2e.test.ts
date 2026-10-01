import { describe, expect, it } from "vitest";
import { aicWhenEnabled, defineSuite, useLease } from "../../src/harness/index.ts";

// Enumeration and collection views on both Java-map surfaces: the registered
// objectAttributes map and JsonValue.object(). Each holds keys named like its
// methods (get, size) next to a plain one. Every value is an exact string, so
// a lane disagreement cannot hide behind a pattern.
function probe(prefix: string): string[] {
  const p = (key: string, body: string) =>
    `nodeState.putShared("${prefix}${key}", t(function () { ${body} }));`;
  return [
    p("ForIn", 'var out = []; for (var k in map) { out.push(String(k)); } return out.sort().join(",");'),
    p("ForInValues", 'var out = []; for (var k in map) { out.push(k + "=" + (typeof map[k])); } return out.sort().join(",");'),
    p("Keys", 'return Object.keys(map).sort().join(",");'),
    p("DotMethodKey", "return typeof map.get;"),
    p("IndexMethodKey", 'return typeof map["get"];'),
    p("IndexPlainKey", 'return map["plain"];'),
    p("MethodWithKey", 'try { map.get("plain"); return "ok"; } catch (e) { return "threw:" + e.name + ":" + /not a function/.test(String(e)); }'),
    p("KeySetType", "return typeof map.keySet;"),
    p("KeySetCall", 'var ks = map.keySet(); return "ok";'),
    p("EntrySet", "return typeof map.entrySet;"),
    p("EntrySetSize", "return map.entrySet().size();"),
    p("EntryHasNext", "return map.entrySet().iterator().hasNext();"),
    p("EntryNext", "return map.entrySet().iterator().next();"),
    p("EntryToArray", "return map.entrySet().toArray().length;"),
    p("EntryElement", "return map.entrySet().toArray()[0];"),
    p("EntryLive", 'var es = map.entrySet(); var before = es.size(); map.put("later2", "l2"); return before + ":" + es.size();'),
    p("Values", "return typeof map.values;"),
    p("ValuesSize", "return map.values().size();"),
    p("ValuesGet", "return typeof map.values().get;"),
    p("ValuesIter", "return map.values().iterator().hasNext();"),
    p("ValuesToArray", 'return Array.prototype.slice.call(map.values().toArray()).sort().join(",");'),
    p("ValuesLive", 'var vs = map.values(); var before = vs.size(); map.put("later3", "l3"); return before + ":" + vs.size();'),
    p("PutExisting", 'return map.put("plain", "p2");'),
    p("PutNew", 'return map.put("fresh", "f");'),
    p("PutMethodName", 'return map.put("size", "s2");'),
  ];
}

const suite = defineSuite({
  name: "java-map-enumeration",
  script: [
    'function t(f) { try { return String(f()); } catch (e) { return "threw:" + String(e).replace(/ \\([^()]*#[0-9]+\\)$/, ""); } }',
    'var map = nodeState.get("objectAttributes");',
    ...probe("reg"),
    'map = org.forgerock.json.JsonValue.object(); map.put("plain", "p"); map.put("get", "g"); map.put("size", "s");',
    ...probe("jv"),
    'action.goTo("done");',
  ].join("\n"),
  outcomes: ["done"],
  always: { registeredObjectAttributes: { plain: "p", get: "g", size: "s" } },
});

// Measured 2026-10-01 on AIC (next-gen), identical on both surfaces.
const shutter = (javaClass: string) =>
  `threw:InternalError: Access to Java class "${javaClass}" is prohibited.`;
const KEYSET = shutter("java.util.LinkedHashMap$LinkedKeySet");
const ENTRY = shutter("java.util.LinkedHashMap$Entry");
const ITERATOR = shutter("java.util.ArrayList$Itr");
const ROW: Record<string, string> = {
  ForIn: "get,plain,size",
  ForInValues: "get=string,plain=string,size=string",
  Keys: "get,plain,size",
  DotMethodKey: "string",
  IndexMethodKey: "string",
  IndexPlainKey: "p",
  MethodWithKey: "threw:TypeError:true",
  KeySetType: "function",
  KeySetCall: KEYSET,
  EntrySet: "function",
  EntrySetSize: "3",
  EntryHasNext: "true",
  EntryNext: ENTRY,
  EntryToArray: "3",
  EntryElement: ENTRY,
  EntryLive: "3:3",
  Values: "function",
  ValuesSize: "4",
  ValuesGet: "function",
  ValuesIter: ITERATOR,
  ValuesToArray: "g,l2,p,s",
  ValuesLive: "4:4",
  PutExisting: "p",
  PutNew: "null",
  PutMethodName: "s",
};
const MEASURED = Object.fromEntries(
  ["reg", "jv"].flatMap((prefix) =>
    Object.entries(ROW).map(([key, value]) => [prefix + key, value])
  )
);

describe("Java map enumeration", () => {
  const lease = useLease(suite, aicWhenEnabled("live-java-map-enumeration"));

  it("enumerates and views both map surfaces as AIC does", async () => {
    const run = await lease.run().expect({
      outcome: "done",
      sharedState: { added: MEASURED, changed: { objectAttributes: { plain: "p2", get: "g", size: "s2", later2: "l2", later3: "l3", fresh: "f" } } },
    });
    expect(run.verdict.summary).toBe("");
    expect(run.verdict.pass).toBe(true);
  });
});
