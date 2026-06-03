// Probe: object destructuring assignment `var { x, y } = src`. Safe to delete.
function emit(payload) {
  if (callbacks.isEmpty()) {
    callbacksBuilder.hiddenValueCallback("result", JSON.stringify(payload));
  }
  outcome = payload.ok ? "ok" : "error";
}

try {
  var src = { x: 1, y: 2 };
  var { x, y } = src;
  emit({ ok: true, feature: "destructuring-object", value: x + "," + y });
} catch (e) {
  emit({ ok: false, feature: "destructuring-object", error: String(e) });
}
