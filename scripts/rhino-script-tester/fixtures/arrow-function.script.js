// Probe: arrow functions. Safe to delete.
// Corpus uses `=>` in 184/384 src scripts, so we expect this to parse; the
// probe confirms it on the live engine.
function emit(payload) {
  if (callbacks.isEmpty()) {
    callbacksBuilder.hiddenValueCallback("result", JSON.stringify(payload));
  }
  outcome = payload.ok ? "ok" : "error";
}

try {
  var dbl = (n) => n * 2;
  var add = (a, b) => {
    return a + b;
  };
  emit({
    ok: true,
    feature: "arrow-function",
    value: dbl(21) + "," + add(40, 2),
  });
} catch (e) {
  emit({ ok: false, feature: "arrow-function", error: String(e) });
}
