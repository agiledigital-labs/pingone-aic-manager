// LIBRARY-context probe for the ES2015 global objects. Uploaded to the sandbox as
// `rhino-lib-es2015-globals-probe` (script id …7407) and consumed by
// lib-es2015-globals-consumer.script.js. Safe to delete.
//
// A LIBRARY script is compiled separately from its caller, so "the globals the
// decision node sees" is not automatically "the globals the library sees" — and a
// Set-based dedupe helper is exactly the kind of code that would live here.
// Probed rather than inferred, and probed for the whole global set so the
// LIBRARY column of the docs/api/12 table has no unexplained gaps.
function probe(name, fn) {
  try {
    return { name: name, ok: true, value: String(fn()) };
  } catch (e) {
    return { name: name, ok: false, error: String(e) };
  }
}

exports.globals = {
  Map: typeof Map,
  Set: typeof Set,
  WeakMap: typeof WeakMap,
  WeakSet: typeof WeakSet,
  Symbol: typeof Symbol,
  Promise: typeof Promise,
  Proxy: typeof Proxy,
  Reflect: typeof Reflect,
  // Control: present everywhere, so an all-"undefined" result cannot be blamed
  // on library scope being probed wrongly.
  JSON: typeof JSON,
};

exports.results = [
  probe("new Map + set/get/size", function () {
    var m = new Map();
    m.set("a", 1);
    return m.get("a") + ":" + m.size;
  }),
  probe("new Set + add/has/size", function () {
    var s = new Set([1, 2, 2]);
    s.add(3);
    return s.has(2) + ":" + s.size;
  }),
  // The object-keyed fallback a library dedupe helper has to use instead.
  probe("object-as-set fallback", function () {
    var seen = {};
    var out = [];
    var items = ["a", "b", "a"];
    for (var i = 0; i < items.length; i++) {
      if (!seen[items[i]]) {
        seen[items[i]] = true;
        out.push(items[i]);
      }
    }
    return out.join(",");
  }),
];
