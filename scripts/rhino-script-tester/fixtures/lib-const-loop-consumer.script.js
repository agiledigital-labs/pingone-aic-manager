// Next-gen scripted decision that require()s the lib-const-loop-probe library
// and reports whether a loop-body `const` inside one of its functions survived.
// Safe to delete.
function emit(payload) {
  if (callbacks.isEmpty()) {
    callbacksBuilder.hiddenValueCallback("result", JSON.stringify(payload));
  }
  outcome = "ok";
}

try {
  var lib = require("rhino-lib-const-loop-probe");
  emit({
    ok: true,
    feature: "lib-const-loop-in-function",
    fromLoopConst: lib.fromLoopConst,
  });
} catch (e) {
  emit({ ok: false, feature: "lib-const-loop-in-function", error: String(e) });
}
