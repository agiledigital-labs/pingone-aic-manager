// Legacy scripted decision probe (evaluatorVersion 1.0). Safe to delete.
// Same question as fixtures/es2017-object.script.js, asked of the legacy engine.
var frJava = JavaImporter(
  org.forgerock.openam.auth.node.api.Action,
  com.sun.identity.authentication.callbacks.HiddenValueCallback
);

function emit(payload) {
  var json = JSON.stringify(payload);
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
    probe("typeof Object.keys (control)", function () {
      return typeof Object.keys;
    })
  );
  results.push(
    probe("Object.keys", function () {
      return Object.keys({ a: 1, b: 2 }).join(",");
    })
  );
  results.push(
    probe("typeof Object.values", function () {
      return typeof Object.values;
    })
  );
  results.push(
    probe("Object.values", function () {
      return JSON.stringify(Object.values({ a: 1, b: "x" }));
    })
  );
  results.push(
    probe("typeof Object.entries", function () {
      return typeof Object.entries;
    })
  );
  results.push(
    probe("Object.entries", function () {
      return JSON.stringify(Object.entries({ a: 1, b: "x" }));
    })
  );
  results.push(
    probe("typeof Object.getOwnPropertyDescriptors", function () {
      return typeof Object.getOwnPropertyDescriptors;
    })
  );
  emit({ ok: true, feature: "legacy-es2017-object", value: results });
} catch (e) {
  emit({ ok: false, feature: "legacy-es2017-object", error: String(e) });
}
