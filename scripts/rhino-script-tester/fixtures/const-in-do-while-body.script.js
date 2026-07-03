// Probe: `const` declared inside a do-while body (re-declared each iteration).
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
  do {
    const doubled = i * 2;
    out.push(doubled);
    i++;
  } while (i < 3);
  emit({ ok: true, feature: "const-in-do-while-body", value: out.join(",") });
} catch (e) {
  emit({ ok: false, feature: "const-in-do-while-body", error: String(e) });
}
