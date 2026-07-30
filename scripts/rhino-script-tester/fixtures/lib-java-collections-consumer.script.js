// Next-gen scripted decision that require()s rhino-lib-java-collections-probe and
// reports which java.util collections are reachable from LIBRARY scope.
// run-probes.sh extracts the HiddenValueCallback value. Safe to delete.
//
// The same constructions are probed directly by java-collections.script.js in
// decision-node scope, so a difference between the two runs isolates the
// allow-list to the compiling context.
function emit(payload) {
  if (callbacks.isEmpty()) {
    callbacksBuilder.hiddenValueCallback("result", JSON.stringify(payload));
  }
  outcome = "ok";
}

try {
  var lib = require("rhino-lib-java-collections-probe");
  emit({
    ok: true,
    feature: "lib-java-collections",
    typeofJavaImporter: lib.typeofJavaImporter,
    typeofJava: lib.typeofJava,
    value: lib.results,
  });
} catch (e) {
  emit({ ok: false, feature: "lib-java-collections", error: String(e) });
}
