// Probe: `const` declared inside a loop body (re-declared each iteration).
// Safe to delete.
function emit(payload) {
  if (callbacks.isEmpty()) {
    callbacksBuilder.hiddenValueCallback("result", JSON.stringify(payload));
  }
  outcome = payload.ok ? "ok" : "error";
}

try {
  var out = [];
  for (var i = 0; i < 3; i++) {
    const doubled = i * 2;
    out.push(doubled);
  }
  emit({ ok: true, feature: "const-in-loop-body", value: out.join(",") });
} catch (e) {
  emit({ ok: false, feature: "const-in-loop-body", error: String(e) });
}
