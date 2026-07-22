// Next-gen scripted decision that require()s the lib-openidm-read-probe
// library and reports whether openidm.read worked from inside library code.
// Safe to delete.
function emit(payload) {
  if (callbacks.isEmpty()) {
    callbacksBuilder.hiddenValueCallback("result", JSON.stringify(payload));
  }
  outcome = "ok";
}

try {
  var lib = require("rhino-lib-openidm-read-probe");
  emit({
    ok: true,
    feature: "lib-openidm-read",
    fromLib: lib.fromLib,
    missResult: lib.missResult,
  });
} catch (e) {
  emit({ ok: false, feature: "lib-openidm-read", error: String(e) });
}
