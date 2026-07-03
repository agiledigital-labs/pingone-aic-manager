// Probe: `const` declared in a nested block inside a loop body.
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
    if (i < 3) {
      const nested = i * 2;
      out.push(nested);
    }
  }
  emit({
    ok: true,
    feature: "const-in-nested-loop-block",
    value: out.join(","),
  });
} catch (e) {
  emit({
    ok: false,
    feature: "const-in-nested-loop-block",
    error: String(e),
  });
}
