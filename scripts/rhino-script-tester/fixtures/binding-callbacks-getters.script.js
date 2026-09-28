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
      callbacksBuilder.textOutputCallback(0, "THREW " + label + ": " + String(e));
    } catch (ignore) {}
  }
}
if (callbacks.isEmpty()) {
  build("stringAttributeInputCallback/4", function () {
    callbacksBuilder.stringAttributeInputCallback("attr", "Prompt", "value", true);
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
    callbacksBuilder.booleanAttributeInputCallback("bool", "Boolean", true, true);
  });
  build("confirmationCallback/4", function () {
    callbacksBuilder.confirmationCallback("Continue?", 0, ["yes", "no"], 0);
  });
  build("languageCallback/2", function () {
    callbacksBuilder.languageCallback("en", "US");
  });
  build("idPCallback/9", function () {
    callbacksBuilder.idPCallback("provider", "client", "https://example.com", ["openid"], "nonce", "", "", [], false);
  });
  build("validatedPasswordCallback/5", function () {
    callbacksBuilder.validatedPasswordCallback("Password", false, {}, false, []);
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
  var r = [];
  try {
    var x = callbacks.getStringAttributeInputCallbacks();
    var first = x && x.length ? x[0] : null;
    r.push({
      name: "getStringAttributeInputCallbacks",
      ok: true,
      list: describe(x),
      first: first === null ? null : describe(first),
    });
  } catch (e) {
    r.push({
      name: "getStringAttributeInputCallbacks",
      ok: false,
      error: String(e),
    });
  }
  try {
    var x = callbacks.getChoiceCallbacks();
    var first = x && x.length ? x[0] : null;
    r.push({
      name: "getChoiceCallbacks",
      ok: true,
      list: describe(x),
      first: first === null ? null : describe(first),
    });
  } catch (e) {
    r.push({ name: "getChoiceCallbacks", ok: false, error: String(e) });
  }
  try {
    var x = callbacks.getNameCallbacks();
    var first = x && x.length ? x[0] : null;
    r.push({
      name: "getNameCallbacks",
      ok: true,
      list: describe(x),
      first: first === null ? null : describe(first),
    });
  } catch (e) {
    r.push({ name: "getNameCallbacks", ok: false, error: String(e) });
  }
  try {
    var x = callbacks.getPasswordCallbacks();
    var first = x && x.length ? x[0] : null;
    r.push({
      name: "getPasswordCallbacks",
      ok: true,
      list: describe(x),
      first: first === null ? null : describe(first),
    });
  } catch (e) {
    r.push({ name: "getPasswordCallbacks", ok: false, error: String(e) });
  }
  try {
    var x = callbacks.getHiddenValueCallbacks();
    var first = x && x.length ? x[0] : null;
    r.push({
      name: "getHiddenValueCallbacks",
      ok: true,
      list: describe(x),
      first: first === null ? null : describe(first),
    });
  } catch (e) {
    r.push({ name: "getHiddenValueCallbacks", ok: false, error: String(e) });
  }
  try {
    var x = callbacks.getTextInputCallbacks();
    var first = x && x.length ? x[0] : null;
    r.push({
      name: "getTextInputCallbacks",
      ok: true,
      list: describe(x),
      first: first === null ? null : describe(first),
    });
  } catch (e) {
    r.push({ name: "getTextInputCallbacks", ok: false, error: String(e) });
  }
  try {
    var x = callbacks.getNumberAttributeInputCallbacks();
    var first = x && x.length ? x[0] : null;
    r.push({
      name: "getNumberAttributeInputCallbacks",
      ok: true,
      list: describe(x),
      first: first === null ? null : describe(first),
    });
  } catch (e) {
    r.push({
      name: "getNumberAttributeInputCallbacks",
      ok: false,
      error: String(e),
    });
  }
  try {
    var x = callbacks.getBooleanAttributeInputCallbacks();
    var first = x && x.length ? x[0] : null;
    r.push({
      name: "getBooleanAttributeInputCallbacks",
      ok: true,
      list: describe(x),
      first: first === null ? null : describe(first),
    });
  } catch (e) {
    r.push({
      name: "getBooleanAttributeInputCallbacks",
      ok: false,
      error: String(e),
    });
  }
  try {
    var x = callbacks.getConfirmationCallbacks();
    var first = x && x.length ? x[0] : null;
    r.push({
      name: "getConfirmationCallbacks",
      ok: true,
      list: describe(x),
      first: first === null ? null : describe(first),
    });
  } catch (e) {
    r.push({ name: "getConfirmationCallbacks", ok: false, error: String(e) });
  }
  try {
    var x = callbacks.getLanguageCallbacks();
    var first = x && x.length ? x[0] : null;
    r.push({
      name: "getLanguageCallbacks",
      ok: true,
      list: describe(x),
      first: first === null ? null : describe(first),
    });
  } catch (e) {
    r.push({ name: "getLanguageCallbacks", ok: false, error: String(e) });
  }
  try {
    var x = callbacks.getIdpCallbacks();
    var first = x && x.length ? x[0] : null;
    r.push({
      name: "getIdpCallbacks",
      ok: true,
      list: describe(x),
      first: first === null ? null : describe(first),
    });
  } catch (e) {
    r.push({ name: "getIdpCallbacks", ok: false, error: String(e) });
  }
  try {
    var x = callbacks.getValidatedPasswordCallbacks();
    var first = x && x.length ? x[0] : null;
    r.push({
      name: "getValidatedPasswordCallbacks",
      ok: true,
      list: describe(x),
      first: first === null ? null : describe(first),
    });
  } catch (e) {
    r.push({
      name: "getValidatedPasswordCallbacks",
      ok: false,
      error: String(e),
    });
  }
  try {
    var x = callbacks.getValidatedUsernameCallbacks();
    var first = x && x.length ? x[0] : null;
    r.push({
      name: "getValidatedUsernameCallbacks",
      ok: true,
      list: describe(x),
      first: first === null ? null : describe(first),
    });
  } catch (e) {
    r.push({
      name: "getValidatedUsernameCallbacks",
      ok: false,
      error: String(e),
    });
  }
  try {
    var x = callbacks.getHttpCallbacks();
    var first = x && x.length ? x[0] : null;
    r.push({
      name: "getHttpCallbacks",
      ok: true,
      list: describe(x),
      first: first === null ? null : describe(first),
    });
  } catch (e) {
    r.push({ name: "getHttpCallbacks", ok: false, error: String(e) });
  }
  try {
    var x = callbacks.getX509CertificateCallbacks();
    var first = x && x.length ? x[0] : null;
    r.push({
      name: "getX509CertificateCallbacks",
      ok: true,
      list: describe(x),
      first: first === null ? null : describe(first),
    });
  } catch (e) {
    r.push({
      name: "getX509CertificateCallbacks",
      ok: false,
      error: String(e),
    });
  }
  try {
    var x = callbacks.getConsentMappingCallbacks();
    var first = x && x.length ? x[0] : null;
    r.push({
      name: "getConsentMappingCallbacks",
      ok: true,
      list: describe(x),
      first: first === null ? null : describe(first),
    });
  } catch (e) {
    r.push({ name: "getConsentMappingCallbacks", ok: false, error: String(e) });
  }
  try {
    var x = callbacks.getDeviceProfileCallbacks();
    var first = x && x.length ? x[0] : null;
    r.push({
      name: "getDeviceProfileCallbacks",
      ok: true,
      list: describe(x),
      first: first === null ? null : describe(first),
    });
  } catch (e) {
    r.push({ name: "getDeviceProfileCallbacks", ok: false, error: String(e) });
  }
  try {
    var x = callbacks.getKbaCreateCallbacks();
    var first = x && x.length ? x[0] : null;
    r.push({
      name: "getKbaCreateCallbacks",
      ok: true,
      list: describe(x),
      first: first === null ? null : describe(first),
    });
  } catch (e) {
    r.push({ name: "getKbaCreateCallbacks", ok: false, error: String(e) });
  }
  try {
    var x = callbacks.getSelectIdPCallbacks();
    var first = x && x.length ? x[0] : null;
    r.push({
      name: "getSelectIdPCallbacks",
      ok: true,
      list: describe(x),
      first: first === null ? null : describe(first),
    });
  } catch (e) {
    r.push({ name: "getSelectIdPCallbacks", ok: false, error: String(e) });
  }
  try {
    var x = callbacks.getTermsAndConditionsCallbacks();
    var first = x && x.length ? x[0] : null;
    r.push({
      name: "getTermsAndConditionsCallbacks",
      ok: true,
      list: describe(x),
      first: first === null ? null : describe(first),
    });
  } catch (e) {
    r.push({
      name: "getTermsAndConditionsCallbacks",
      ok: false,
      error: String(e),
    });
  }
  try {
    r.push({ name: "isEmpty", ok: true, value: callbacks.isEmpty() });
  } catch (e) {
    r.push({ name: "isEmpty", ok: false, error: String(e) });
  }
  callbacksBuilder.hiddenValueCallback("result", JSON.stringify(r));
  outcome = "ok";
}
