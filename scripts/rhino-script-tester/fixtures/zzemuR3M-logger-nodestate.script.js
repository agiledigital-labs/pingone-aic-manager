// Next-gen scripted decision. The library reads nodeState, a caller-only
// binding. The expected result is the ReferenceError already recorded for
// library scope. Safe to delete.
function emit(payload) {
  if (callbacks.isEmpty()) {
    callbacksBuilder.hiddenValueCallback("result", JSON.stringify(payload));
  }
  outcome = "ok";
}

var out = { feature: "logger-nodestate" };
try {
  var lib = require("zzemuR3M-logger-lib");
  out.value = lib.readCallerOnly();
  out.ok = true;
} catch (e) {
  out.ok = false;
  out.error = String(e).slice(0, 400);
  out.name = e && e.name ? String(e.name) : "";
}
emit(out);
