// Probe: `const` in a C-style for-loop initializer. Safe to delete.
// Distinguishes parse failure (no callback) from runtime throw (ok:false,
// e.g. assignment-to-const at `i++`) from working (ok:true).
function emit(payload) {
  if (callbacks.isEmpty()) {
    callbacksBuilder.hiddenValueCallback("result", JSON.stringify(payload));
  }
  outcome = payload.ok ? "ok" : "error";
}

try {
  var seen = [];
  for (const i = 0; i < 3; i++) {
    seen.push(i);
  }
  emit({ ok: true, feature: "const-in-for-init", value: seen.join(",") });
} catch (e) {
  emit({ ok: false, feature: "const-in-for-init", error: String(e) });
}
