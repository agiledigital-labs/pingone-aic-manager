// Next-gen probe: enumerate member names of callbacksBuilder + utils via Rhino
// for-in (reflection via getClass() is blocked in the next-gen sandbox). Also
// typeof-probes a list of candidate callback-builder method names from the docs.
// Safe to delete; non-destructive.
function emit(payload) {
  if (callbacks.isEmpty()) {
    callbacksBuilder.hiddenValueCallback("result", JSON.stringify(payload));
  }
  outcome = "ok";
}

function enumerate(obj) {
  var out = [];
  for (var k in obj) {
    out.push(String(k));
  }
  out.sort();
  return out;
}

try {
  var candidates = [
    "nameCallback",
    "passwordCallback",
    "hiddenValueCallback",
    "textInputCallback",
    "textOutputCallback",
    "scriptTextOutputCallback",
    "confirmationCallback",
    "choiceCallback",
    "pollingWaitCallback",
    "suspendedTextOutputCallback",
    "stringAttributeInputCallback",
    "numberAttributeInputCallback",
    "booleanAttributeInputCallback",
    "redirectCallback",
    "metadataCallback",
    "pingOneProtectInitializeCallback",
    "pingOneProtectEvaluationCallback",
    "deviceProfileCallback",
    "selectIdPCallback",
    "consentMappingCallback",
    "kbaCreateCallback",
    "termsAndConditionsCallback",
    "validatedPasswordCallback",
    "validatedUsernameCallback",
  ];
  var present = {};
  for (var i = 0; i < candidates.length; i++) {
    present[candidates[i]] = typeof callbacksBuilder[candidates[i]];
  }
  emit({
    ok: true,
    feature: "enum-callbacks-utils",
    callbacksBuilderEnum: enumerate(callbacksBuilder),
    utilsEnum: enumerate(utils),
    candidateTypeof: present,
  });
} catch (e) {
  emit({ ok: false, feature: "enum-callbacks-utils", error: String(e) });
}
