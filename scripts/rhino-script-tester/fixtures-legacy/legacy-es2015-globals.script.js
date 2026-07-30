// Legacy scripted decision probe (evaluatorVersion 1.0). Safe to delete.
//
// Same ES2015-collection-globals question as fixtures/map-set.script.js, asked of
// the LEGACY engine: if both engines answer identically, Map/Set availability is
// a property of AM's Rhino configuration and not of the engine generation.
// Legacy has no callbacksBuilder, so results go out via the classic
// JavaImporter + Action.send(HiddenValueCallback) path.
var frJava = JavaImporter(
  org.forgerock.openam.auth.node.api.Action,
  com.sun.identity.authentication.callbacks.HiddenValueCallback
);

function emit(payload) {
  var json = JSON.stringify(payload);
  if (typeof callbacksBuilder !== "undefined" && callbacks.isEmpty()) {
    callbacksBuilder.hiddenValueCallback("result", json);
    outcome = "ok";
    return;
  }
  if (callbacks.isEmpty()) {
    action = frJava.Action.send(
      new frJava.HiddenValueCallback("result", json)
    ).build();
  } else {
    action = frJava.Action.goTo("ok").build();
  }
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
  // Legacy can reach java.util.HashMap/HashSet through JavaImporter, which is the
  // idiomatic legacy substitute — worth knowing whether it is available.
  results.push(
    probe("java.util.HashMap", function () {
      var jm = new java.util.HashMap();
      jm.put("a", 1);
      return jm.get("a") + ":" + jm.size();
    })
  );
  emit({ ok: true, feature: "legacy-es2015-globals", value: results });
} catch (e) {
  emit({ ok: false, feature: "legacy-es2015-globals", error: String(e) });
}
