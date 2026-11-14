// Next-gen scripted decision. Same library call as the shadow probe, but the
// caller names its variable `log`, so the library's `logger` is not shadowed.
// Safe to delete.
function emit(payload) {
  if (callbacks.isEmpty()) {
    callbacksBuilder.hiddenValueCallback("result", JSON.stringify(payload));
  }
  outcome = "ok";
}

var out = { feature: "logger-noshadow" };
try {
  var log = require("zzemuR3M-logger-lib").load();
  log.info("zzemuR3M-noshadow");
  out.ok = true;
  out.result = "returned";
} catch (e) {
  out.ok = false;
  out.error = String(e).slice(0, 400);
  out.name = e && e.name ? String(e.name) : "";
}
emit(out);
