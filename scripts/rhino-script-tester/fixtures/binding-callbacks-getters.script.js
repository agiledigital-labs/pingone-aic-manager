// Probe: callback getters across two authentication visits. Safe to delete.
// The runner must use RESUBMIT=1 to submit visit one defaults unchanged.
function describe(v) {
  var r = { type: typeof v, string: String(v) };
  try {
    r.array = Array.isArray(v);
  } catch (e) {}
  try {
    r.keys = Object.keys(v);
  } catch (e) {}
  try {
    r.length = v.length;
  } catch (e) {}
  try {
    r.size = v.size();
  } catch (e) {}
  return r;
}
function build(label, fn) {
  try {
    fn();
  } catch (e) {
    try {
      callbacksBuilder.textOutputCallback(
        0,
        "THREW " + label + ": " + String(e)
      );
    } catch (ignore) {}
  }
}
if (callbacks.isEmpty()) {
  build("stringAttributeInputCallback/4", function () {
    callbacksBuilder.stringAttributeInputCallback(
      "attr",
      "Prompt",
      "value",
      true
    );
  });
  build("choiceCallback/4", function () {
    callbacksBuilder.choiceCallback("Prompt", ["one", "two"], 0, false);
  });
  build("nameCallback/2", function () {
    callbacksBuilder.nameCallback("Name", "Ada");
  });
  build("passwordCallback/2", function () {
    callbacksBuilder.passwordCallback("Password", false);
  });
  build("hiddenValueCallback/2", function () {
    callbacksBuilder.hiddenValueCallback("hidden", "value");
  });
  build("textInputCallback/2", function () {
    callbacksBuilder.textInputCallback("Prompt", "default");
  });
  build("numberAttributeInputCallback/4", function () {
    callbacksBuilder.numberAttributeInputCallback("num", "Number", 7, true);
  });
  build("booleanAttributeInputCallback/4", function () {
    callbacksBuilder.booleanAttributeInputCallback(
      "bool",
      "Boolean",
      true,
      true
    );
  });
  build("confirmationCallback/4", function () {
    callbacksBuilder.confirmationCallback("Continue?", 0, ["yes", "no"], 0);
  });
  build("languageCallback/2", function () {
    callbacksBuilder.languageCallback("en", "US");
  });
  build("idPCallback/9", function () {
    callbacksBuilder.idPCallback(
      "provider",
      "client",
      "https://example.com",
      ["openid"],
      "nonce",
      "",
      "",
      [],
      false
    );
  });
  build("validatedPasswordCallback/5", function () {
    callbacksBuilder.validatedPasswordCallback(
      "Password",
      false,
      {},
      false,
      []
    );
  });
  build("validatedUsernameCallback/4", function () {
    callbacksBuilder.validatedUsernameCallback("Username", {}, false, []);
  });
  build("httpCallback/3", function () {
    callbacksBuilder.httpCallback("auth", "nego", "");
  });
  build("x509CertificateCallback/2", function () {
    callbacksBuilder.x509CertificateCallback("certificate", "prompt");
  });
  build("consentMappingCallback/3", function () {
    callbacksBuilder.consentMappingCallback({}, "Consent", true);
  });
  build("deviceProfileCallback/3", function () {
    callbacksBuilder.deviceProfileCallback(true, true, "Device");
  });
  build("kbaCreateCallback/3", function () {
    callbacksBuilder.kbaCreateCallback("Question", ["Q"], true);
  });
  build("selectIdPCallback/1", function () {
    callbacksBuilder.selectIdPCallback({});
  });
  build("termsAndConditionsCallback/3", function () {
    callbacksBuilder.termsAndConditionsCallback("v1", "terms", "2026-01-01");
  });
  outcome = "ok";
} else {
  // Every getter in the binding inventory, in inventory order.
  var GETTERS = [
    "getStringAttributeInputCallbacks",
    "getChoiceCallbacks",
    "getNameCallbacks",
    "getPasswordCallbacks",
    "getHiddenValueCallbacks",
    "getTextInputCallbacks",
    "getNumberAttributeInputCallbacks",
    "getBooleanAttributeInputCallbacks",
    "getConfirmationCallbacks",
    "getLanguageCallbacks",
    "getIdpCallbacks",
    "getValidatedPasswordCallbacks",
    "getValidatedUsernameCallbacks",
    "getHttpCallbacks",
    "getX509CertificateCallbacks",
    "getConsentMappingCallbacks",
    "getDeviceProfileCallbacks",
    "getKbaCreateCallbacks",
    "getSelectIdPCallbacks",
    "getTermsAndConditionsCallbacks",
  ];
  var r = [];
  for (var i = 0; i < GETTERS.length; i++) {
    try {
      var list = callbacks[GETTERS[i]]();
      var first = null;
      if (list !== null && typeof list.size === "function") {
        first = list.size() > 0 ? describe(list.get(0)) : null;
      } else if (list !== null && list.length > 0) {
        first = describe(list[0]);
      }
      r.push({
        name: GETTERS[i],
        ok: true,
        list: describe(list),
        first: first,
      });
    } catch (e) {
      r.push({ name: GETTERS[i], ok: false, error: String(e) });
    }
  }
  try {
    r.push({ name: "isEmpty", ok: true, value: callbacks.isEmpty() });
  } catch (e) {
    r.push({ name: "isEmpty", ok: false, error: String(e) });
  }
  callbacksBuilder.hiddenValueCallback("result", JSON.stringify(r));
  outcome = "ok";
}
