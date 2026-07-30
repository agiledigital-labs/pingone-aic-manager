// Probe: ES2015 *global objects* — Map, Set, WeakMap, WeakSet, Symbol, Proxy,
// Reflect, Promise — on the next-gen engine. Deliberately the companion to
// es2015-methods.script.js: that fixture probes ES2015 *prototype methods* (all
// of which work), this one probes the new global constructors. The two answers
// differ, so they get separate fixtures. Safe to delete.
//
// `typeof X` never throws, so presence and usability are probed separately: a
// constructor can be present but unusable, and `new X()` on a missing global
// raises ReferenceError rather than a parse error.
//
// These rows also decide the AM `lib` list in the script workspace's tsconfig:
// TypeScript's ES2015 lib declares all of these, so any that are missing at
// runtime must be excluded from `lib` or tsc will happily accept code that
// ReferenceErrors in the tenant.
function emit(payload) {
  if (callbacks.isEmpty()) {
    callbacksBuilder.hiddenValueCallback("result", JSON.stringify(payload));
  }
  outcome = payload.ok ? "ok" : "error";
}

function probe(name, fn) {
  try {
    return { name: name, ok: true, value: String(fn()) };
  } catch (e) {
    return { name: name, ok: false, error: String(e) };
  }
}

try {
  var results = [];
  results.push(
    probe("typeof Map", function () {
      return typeof Map;
    })
  );
  results.push(
    probe("typeof Set", function () {
      return typeof Set;
    })
  );
  results.push(
    probe("typeof WeakMap", function () {
      return typeof WeakMap;
    })
  );
  results.push(
    probe("typeof WeakSet", function () {
      return typeof WeakSet;
    })
  );
  results.push(
    probe("typeof Symbol", function () {
      return typeof Symbol;
    })
  );
  results.push(
    probe("typeof Proxy", function () {
      return typeof Proxy;
    })
  );
  results.push(
    probe("typeof Reflect", function () {
      return typeof Reflect;
    })
  );
  results.push(
    probe("typeof Promise", function () {
      return typeof Promise;
    })
  );
  // Control: a global that is definitely present, so an all-"undefined" result
  // cannot be blamed on the probe harness.
  results.push(
    probe("typeof JSON", function () {
      return typeof JSON;
    })
  );
  results.push(
    probe("new Map + set/get/size", function () {
      var m = new Map();
      m.set("a", 1);
      return m.get("a") + ":" + m.size;
    })
  );
  results.push(
    probe("new Set + add/has/size", function () {
      var s = new Set([1, 2, 2]);
      s.add(3);
      return s.has(2) + ":" + s.size;
    })
  );
  // The documented substitute if the collections are missing: a plain object for
  // string keys. Proves the fallback we would lint people toward actually works.
  results.push(
    probe("object-as-map fallback", function () {
      var o = {};
      o["a"] = 1;
      return o["a"] + ":" + Object.keys(o).length;
    })
  );
  emit({ ok: true, feature: "es2015-globals", value: results });
} catch (e) {
  emit({ ok: false, feature: "es2015-globals", error: String(e) });
}
