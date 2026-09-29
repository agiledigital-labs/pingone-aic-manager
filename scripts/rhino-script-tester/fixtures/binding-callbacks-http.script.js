// Probe: callbacksBuilder.httpCallback, both overloads. Safe to delete.
// AM cannot serialise an HttpCallback to REST JSON; measured 2026-09-29 as a 400
// for the whole /authenticate response, so it is probed on its own.
function build(label, fn) {
  try {
    fn();
  } catch (e) {
    try {
      callbacksBuilder.textOutputCallback(
        0,
        "THREW " + label + ": " + String(e)
      );
    } catch (ignore) {}
  }
}
build("httpCallback/3", function () {
  callbacksBuilder.httpCallback("probe-210", "probe-211", "probe-212");
});
build("httpCallback/4", function () {
  callbacksBuilder.httpCallback("probe-220", "probe-221", "probe-222", 401);
});
outcome = "ok";
