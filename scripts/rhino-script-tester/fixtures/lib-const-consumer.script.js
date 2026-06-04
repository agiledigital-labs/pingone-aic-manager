// Next-gen scripted decision that require()s the lib-const-probe library and
// reports whether its top-level const survived. Safe to delete.
function emit(payload) {
  if (callbacks.isEmpty()) {
    callbacksBuilder.hiddenValueCallback("result", JSON.stringify(payload));
  }
  outcome = "ok";
}

try {
  var lib = require("rhino-lib-const-probe");
  emit({
    ok: true,
    feature: "lib-top-const",
    fromConst: lib.fromConst,
    fromVar: lib.fromVar,
  });
} catch (e) {
  emit({ ok: false, feature: "lib-top-const", error: String(e) });
}
