// Probe: `const` declared inside a loop body, where the loop lives inside a
// function (the existing const-in-loop-body probe has its loop at script top
// level — this isolates whether function scope changes the behavior).
// Safe to delete.
function emit(payload) {
  if (callbacks.isEmpty()) {
    callbacksBuilder.hiddenValueCallback("result", JSON.stringify(payload));
  }
  outcome = payload.ok ? "ok" : "error";
}

function doubleAll() {
  var out = [];
  for (var i = 0; i < 3; i++) {
    const doubled = i * 2;
    out.push(doubled);
  }
  return out.join(",");
}

try {
  emit({ ok: true, feature: "const-in-loop-in-function", value: doubleAll() });
} catch (e) {
  emit({ ok: false, feature: "const-in-loop-in-function", error: String(e) });
}
