// Probe: default function parameters. Safe to delete.
function emit(payload) {
  if (callbacks.isEmpty()) {
    callbacksBuilder.hiddenValueCallback("result", JSON.stringify(payload));
  }
  outcome = payload.ok ? "ok" : "error";
}

function withDefault(a, b = 2) {
  return a + b;
}

try {
  emit({
    ok: true,
    feature: "default-params",
    value: withDefault(40) + "," + withDefault(40, 10),
  });
} catch (e) {
  emit({ ok: false, feature: "default-params", error: String(e) });
}
