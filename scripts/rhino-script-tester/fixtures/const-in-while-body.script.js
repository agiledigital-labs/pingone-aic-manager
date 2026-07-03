// Probe: `const` declared inside a while-loop body (re-declared each iteration).
// Safe to delete.
function emit(payload) {
  if (callbacks.isEmpty()) {
    callbacksBuilder.hiddenValueCallback("result", JSON.stringify(payload));
  }
  outcome = payload.ok ? "ok" : "error";
}

try {
  var out = [];
  var i = 0;
  while (i < 3) {
    const doubled = i * 2;
    out.push(doubled);
    i++;
  }
  emit({ ok: true, feature: "const-in-while-body", value: out.join(",") });
} catch (e) {
  emit({ ok: false, feature: "const-in-while-body", error: String(e) });
}
