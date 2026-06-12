// Probe: repeated `const` of the same name across SIBLING (non-nested) blocks
// inside one function. Claim under test: Rhino requires `const` to be unique
// per function, so this should be a parse error (no callback / HTTP 401) even
// though the two declarations don't shadow each other. Safe to delete.
function emit(payload) {
  if (callbacks.isEmpty()) {
    callbacksBuilder.hiddenValueCallback("result", JSON.stringify(payload));
  }
  outcome = payload.ok ? "ok" : "error";
}

function run() {
  var out = [];
  {
    const dup = "first";
    out.push(dup);
  }
  {
    const dup = "second";
    out.push(dup);
  }
  return out.join(",");
}

try {
  emit({ ok: true, feature: "const-dup-across-blocks", value: run() });
} catch (e) {
  emit({ ok: false, feature: "const-dup-across-blocks", error: String(e) });
}
