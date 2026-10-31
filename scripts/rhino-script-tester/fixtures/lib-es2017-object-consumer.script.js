// Next-gen scripted decision that require()s rhino-lib-es2017-object-probe and
// reports which ES2017 Object members exist inside LIBRARY scope. Safe to delete.
function emit(payload) {
  if (callbacks.isEmpty()) {
    callbacksBuilder.hiddenValueCallback("result", JSON.stringify(payload));
  }
  outcome = "ok";
}

try {
  var lib = require("rhino-lib-es2017-object-probe");
  emit({ ok: true, feature: "lib-es2017-object", value: lib.results });
} catch (e) {
  emit({ ok: false, feature: "lib-es2017-object", error: String(e) });
}
