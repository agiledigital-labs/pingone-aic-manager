// Probe: template literals. Safe to delete.
// Corpus uses backtick templates in 181/384 src scripts.
function emit(payload) {
  if (callbacks.isEmpty()) {
    callbacksBuilder.hiddenValueCallback("result", JSON.stringify(payload));
  }
  outcome = payload.ok ? "ok" : "error";
}

try {
  var who = "world";
  var n = 42;
  var s = `hi ${who} ${n}`;
  emit({ ok: true, feature: "template-literal", value: s });
} catch (e) {
  emit({ ok: false, feature: "template-literal", error: String(e) });
}
