// Consumer for rhino-lib-openidm-miss-probe: reports openidm.read miss behavior
// for the IDR scorer's managed-object paths. Safe to delete.
function emit(payload) {
  if (callbacks.isEmpty()) {
    callbacksBuilder.hiddenValueCallback("result", JSON.stringify(payload));
  }
  outcome = "ok";
}

try {
  var lib = require("rhino-lib-openidm-miss-probe");
  emit({
    ok: true,
    feature: "lib-openidm-miss",
    variantMiss: lib.variantMiss,
    discrepancyMiss: lib.discrepancyMiss,
    unknownTypeMiss: lib.unknownTypeMiss,
  });
} catch (e) {
  emit({ ok: false, feature: "lib-openidm-miss", error: String(e) });
}
