// Next-gen scripted decision that require()s rhino-lib-array-fill-probe and
// reports whether `new Array(n).fill()` and `Array.from({length}, () => …)`
// work in LIBRARY scope. run-probes.sh extracts the HiddenValueCallback value.
// Safe to delete.
function emit(payload) {
  if (callbacks.isEmpty()) {
    callbacksBuilder.hiddenValueCallback("result", JSON.stringify(payload));
  }
  outcome = "ok";
}

try {
  var lib = require("rhino-lib-array-fill-probe");
  emit({
    ok: true,
    feature: "lib-array-fill-from",
    fill: lib.fill,
    from: lib.from,
  });
} catch (e) {
  emit({ ok: false, feature: "lib-array-fill-from", error: String(e) });
}
