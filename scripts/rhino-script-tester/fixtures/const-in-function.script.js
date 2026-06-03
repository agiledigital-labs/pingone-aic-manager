// Probe: `const` inside a function body. Safe to delete.
function emit(payload) {
  if (callbacks.isEmpty()) {
    callbacksBuilder.hiddenValueCallback("result", JSON.stringify(payload));
  }
  outcome = payload.ok ? "ok" : "error";
}

function useConst() {
  const FN_SCOPED = "const-in-function-ok";
  return FN_SCOPED;
}

try {
  emit({ ok: true, feature: "const-in-function", value: useConst() });
} catch (e) {
  emit({ ok: false, feature: "const-in-function", error: String(e) });
}
