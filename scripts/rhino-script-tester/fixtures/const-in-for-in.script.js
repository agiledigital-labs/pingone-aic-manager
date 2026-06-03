// Probe: `const` in a for...in loop (a legitimate ES6 use). Safe to delete.
function emit(payload) {
  if (callbacks.isEmpty()) {
    callbacksBuilder.hiddenValueCallback("result", JSON.stringify(payload));
  }
  outcome = payload.ok ? "ok" : "error";
}

try {
  var obj = { a: 1, b: 2 };
  var keys = [];
  for (const k in obj) {
    keys.push(k);
  }
  emit({ ok: true, feature: "const-in-for-in", value: keys.join(",") });
} catch (e) {
  emit({ ok: false, feature: "const-in-for-in", error: String(e) });
}
