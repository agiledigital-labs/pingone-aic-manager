// Next-gen scripted decision. Rebinds `logger` to the library's load() result
// and calls info. A returned callback means the call came back; the log line
// count says whose logger ran. Safe to delete.
function emit(payload) {
  if (callbacks.isEmpty()) {
    callbacksBuilder.hiddenValueCallback("result", JSON.stringify(payload));
  }
  outcome = "ok";
}

var out = { feature: "logger-shadow" };
try {
  var logger = require("zzemuR3M-logger-lib").load();
  logger.info("zzemuR3M-shadow");
  out.ok = true;
  out.result = "returned";
} catch (e) {
  out.ok = false;
  out.error = String(e).slice(0, 400);
  out.name = e && e.name ? String(e.name) : "";
}
emit(out);
