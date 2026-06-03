// Probe: `const` in a for...of loop. Safe to delete.
function emit(payload) {
  if (callbacks.isEmpty()) {
    callbacksBuilder.hiddenValueCallback("result", JSON.stringify(payload));
  }
  outcome = payload.ok ? "ok" : "error";
}

try {
  var arr = [10, 20, 30];
  var vals = [];
  for (const v of arr) {
    vals.push(v);
  }
  emit({ ok: true, feature: "const-in-for-of", value: vals.join(",") });
} catch (e) {
  emit({ ok: false, feature: "const-in-for-of", error: String(e) });
}
