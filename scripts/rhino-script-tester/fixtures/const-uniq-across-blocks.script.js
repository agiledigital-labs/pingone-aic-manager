// Control for const-dup-across-blocks: identical structure (two sibling blocks
// inside a function, each declaring a block-scoped const) but with DISTINCT
// names. If this parses (ok:true) while the dup variant 401s, the failure is
// attributable to the repeated const name, not the bare-block layout itself.
// Safe to delete.
function emit(payload) {
  if (callbacks.isEmpty()) {
    callbacksBuilder.hiddenValueCallback("result", JSON.stringify(payload));
  }
  outcome = payload.ok ? "ok" : "error";
}

function run() {
  var out = [];
  {
    const first = "first";
    out.push(first);
  }
  {
    const second = "second";
    out.push(second);
  }
  return out.join(",");
}

try {
  emit({ ok: true, feature: "const-uniq-across-blocks", value: run() });
} catch (e) {
  emit({ ok: false, feature: "const-uniq-across-blocks", error: String(e) });
}
