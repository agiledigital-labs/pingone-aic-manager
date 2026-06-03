// Probe: ES6 object property shorthand `{ a, b }`. Safe to delete.
function emit(payload) {
  if (callbacks.isEmpty()) {
    callbacksBuilder.hiddenValueCallback("result", JSON.stringify(payload));
  }
  outcome = payload.ok ? "ok" : "error";
}

try {
  var a = 1;
  var b = 2;
  var obj = { a, b };
  emit({ ok: true, feature: "object-shorthand", value: JSON.stringify(obj) });
} catch (e) {
  emit({ ok: false, feature: "object-shorthand", error: String(e) });
}
