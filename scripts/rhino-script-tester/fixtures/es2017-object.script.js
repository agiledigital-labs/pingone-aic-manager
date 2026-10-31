// Probe: ES2017 Object members on the AM next-gen engine. Safe to delete.
// Object.values / Object.entries are present on the IDM engine (docs/api/12);
// this asks whether the AM engine has them too. Object.keys is the control: it
// is known present, so an all-missing result cannot be blamed on the probe.
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
  emit({ ok: true, feature: "es2017-object", value: results });
} catch (e) {
  emit({ ok: false, feature: "es2017-object", error: String(e) });
}
