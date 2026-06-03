// Probe: `const` at top level. Safe to delete.
// Parse failure (no callback) means Rhino rejects top-level const.
const TOP_LEVEL_CONST = "const-top-level-ok";

function emit(payload) {
  if (callbacks.isEmpty()) {
    callbacksBuilder.hiddenValueCallback("result", JSON.stringify(payload));
  }
  outcome = payload.ok ? "ok" : "error";
}

try {
  emit({ ok: true, feature: "const-top-level", value: TOP_LEVEL_CONST });
} catch (e) {
  emit({ ok: false, feature: "const-top-level", error: String(e) });
}
