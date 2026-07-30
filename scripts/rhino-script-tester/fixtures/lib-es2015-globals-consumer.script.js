// Next-gen scripted decision that require()s rhino-lib-es2015-globals-probe and
// reports which ES2015 global objects exist inside LIBRARY scope. run-probes.sh
// extracts the HiddenValueCallback value. Safe to delete.
function emit(payload) {
  if (callbacks.isEmpty()) {
    callbacksBuilder.hiddenValueCallback("result", JSON.stringify(payload));
  }
  outcome = "ok";
}

try {
  var lib = require("rhino-lib-es2015-globals-probe");
  emit({
    ok: true,
    feature: "lib-es2015-globals",
    globals: lib.globals,
    value: lib.results,
  });
} catch (e) {
  emit({ ok: false, feature: "lib-es2015-globals", error: String(e) });
}
