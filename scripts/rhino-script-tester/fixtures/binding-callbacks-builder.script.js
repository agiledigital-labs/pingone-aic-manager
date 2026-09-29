// Probe: distinct callbacksBuilder JavaScript signatures in the binding inventory. Safe to delete.
// Same-arity overload entries with identical JS types collapse to one callable signature below.
// radioChoiceCallback/3 (3 JSON entries), choiceCallback/4 (3), confirmationCallback/3 (3),
// confirmationCallback/4 (3) are each invoked once; Rhino cannot select their Java-only overloads.
// httpCallback and x509CertificateCallback are in binding-callbacks-http /
// binding-callbacks-x509: AM cannot render either as REST JSON and fails the
// whole /authenticate response with a 400.
// Emits only accumulated callbacks; it does not route the journey.
// Numeric arguments are legal values (message types 0-2, option types 0-2,
// in-range default indexes): an illegal one throws in the Java constructor and
// would record the exception instead of the callback's output fields.
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
build("radioChoiceCallback/3", function () {
  callbacksBuilder.radioChoiceCallback("probe-0", ["left-1", "right-1"], 1);
});
build("suspendedTextOutputCallback/2", function () {
  callbacksBuilder.suspendedTextOutputCallback(2, "probe-11");
});
build("textInputCallback/2", function () {
  callbacksBuilder.textInputCallback("probe-20", "probe-21");
});
build("textInputCallback/1", function () {
  callbacksBuilder.textInputCallback("probe-30");
});
build("scriptTextOutputCallback/1", function () {
  callbacksBuilder.scriptTextOutputCallback("probe-40");
});
build("metadataCallback/1", function () {
  callbacksBuilder.metadataCallback({ marker: "object-50" });
});
build("stringAttributeInputCallback/5", function () {
  callbacksBuilder.stringAttributeInputCallback(
    "probe-60",
    "probe-61",
    "probe-62",
    false,
    ["left-64", "right-64"]
  );
});
build("stringAttributeInputCallback/6", function () {
  callbacksBuilder.stringAttributeInputCallback(
    "probe-70",
    "probe-71",
    "probe-72",
    false,
    { marker: "object-74" },
    false
  );
});
build("stringAttributeInputCallback/4", function () {
  callbacksBuilder.stringAttributeInputCallback(
    "probe-80",
    "probe-81",
    "probe-82",
    false
  );
});
build("stringAttributeInputCallback/7", function () {
  callbacksBuilder.stringAttributeInputCallback(
    "probe-90",
    "probe-91",
    "probe-92",
    false,
    { marker: "object-94" },
    false,
    ["left-96", "right-96"]
  );
});
build("numberAttributeInputCallback/6", function () {
  callbacksBuilder.numberAttributeInputCallback(
    "probe-100",
    "probe-101",
    103,
    false,
    { marker: "object-104" },
    false
  );
});
build("numberAttributeInputCallback/5", function () {
  callbacksBuilder.numberAttributeInputCallback(
    "probe-110",
    "probe-111",
    113,
    false,
    ["left-114", "right-114"]
  );
});
build("numberAttributeInputCallback/4", function () {
  callbacksBuilder.numberAttributeInputCallback(
    "probe-120",
    "probe-121",
    123,
    false
  );
});
build("numberAttributeInputCallback/7", function () {
  callbacksBuilder.numberAttributeInputCallback(
    "probe-130",
    "probe-131",
    133,
    false,
    { marker: "object-134" },
    false,
    ["left-136", "right-136"]
  );
});
build("booleanAttributeInputCallback/5", function () {
  callbacksBuilder.booleanAttributeInputCallback(
    "probe-140",
    "probe-141",
    true,
    false,
    ["left-144", "right-144"]
  );
});
build("booleanAttributeInputCallback/7", function () {
  callbacksBuilder.booleanAttributeInputCallback(
    "probe-150",
    "probe-151",
    true,
    false,
    { marker: "object-154" },
    false,
    ["left-156", "right-156"]
  );
});
build("booleanAttributeInputCallback/4", function () {
  callbacksBuilder.booleanAttributeInputCallback(
    "probe-160",
    "probe-161",
    true,
    false
  );
});
build("booleanAttributeInputCallback/6", function () {
  callbacksBuilder.booleanAttributeInputCallback(
    "probe-170",
    "probe-171",
    true,
    false,
    { marker: "object-174" },
    false
  );
});
build("languageCallback/2", function () {
  callbacksBuilder.languageCallback("probe-180", "probe-181");
});
build("idPCallback/9", function () {
  callbacksBuilder.idPCallback(
    "probe-190",
    "probe-191",
    "probe-192",
    ["left-193", "right-193"],
    "probe-194",
    "probe-195",
    "probe-196",
    ["left-197", "right-197"],
    true
  );
});
build("idPCallback/11", function () {
  callbacksBuilder.idPCallback(
    "probe-200",
    "probe-201",
    "probe-202",
    ["left-203", "right-203"],
    "probe-204",
    "probe-205",
    "probe-206",
    ["left-207", "right-207"],
    true,
    "probe-209",
    "probe-210"
  );
});
build("consentMappingCallback/7", function () {
  callbacksBuilder.consentMappingCallback(
    "probe-260",
    "probe-261",
    "probe-262",
    "probe-263",
    ["left-264", "right-264"],
    "probe-265",
    true
  );
});
build("consentMappingCallback/3", function () {
  // The config needs a `fields` list (without one: "Cannot invoke
  // List.stream()"); its elements come back null in the REST output.
  callbacksBuilder.consentMappingCallback(
    {
      name: "probe-270",
      displayName: "probe-272",
      icon: "probe-273",
      accessLevel: "probe-274",
      fields: ["probe-275"],
    },
    "probe-271",
    true
  );
});
build("deviceProfileCallback/3", function () {
  callbacksBuilder.deviceProfileCallback(true, false, "probe-282");
});
build("kbaCreateCallback/3", function () {
  callbacksBuilder.kbaCreateCallback(
    "probe-290",
    ["left-291", "right-291"],
    true
  );
});
build("selectIdPCallback/1", function () {
  callbacksBuilder.selectIdPCallback({ marker: "object-300" });
});
build("termsAndConditionsCallback/3", function () {
  callbacksBuilder.termsAndConditionsCallback(
    "probe-310",
    "probe-311",
    "probe-312"
  );
});
build("choiceCallback/4", function () {
  callbacksBuilder.choiceCallback(
    "probe-320",
    ["left-321", "right-321"],
    1,
    false
  );
});
build("passwordCallback/2", function () {
  callbacksBuilder.passwordCallback("probe-330", false);
});
build("nameCallback/2", function () {
  callbacksBuilder.nameCallback("probe-340", "probe-341");
});
build("nameCallback/1", function () {
  callbacksBuilder.nameCallback("probe-350");
});
build("hiddenValueCallback/2", function () {
  callbacksBuilder.hiddenValueCallback("probe-360", "probe-361");
});
build("redirectCallback/5", function () {
  callbacksBuilder.redirectCallback(
    "probe-370",
    { marker: "object-371" },
    "probe-372",
    "probe-373",
    "probe-374"
  );
});
build("redirectCallback/4", function () {
  callbacksBuilder.redirectCallback(
    "probe-380",
    { marker: "object-381" },
    "probe-382",
    false
  );
});
build("redirectCallback/3", function () {
  callbacksBuilder.redirectCallback(
    "probe-390",
    { marker: "object-391" },
    "probe-392"
  );
});
build("redirectCallback/6", function () {
  callbacksBuilder.redirectCallback(
    "probe-400",
    { marker: "object-401" },
    "probe-402",
    "probe-403",
    "probe-404",
    false
  );
});
build("confirmationCallback/3 numbers", function () {
  callbacksBuilder.confirmationCallback(1, 0, 1);
});
build("confirmationCallback/3 array", function () {
  callbacksBuilder.confirmationCallback(2, ["left-421", "right-421"], 1);
});
build("confirmationCallback/4 numbers", function () {
  callbacksBuilder.confirmationCallback("probe-430", 0, 2, 3);
});
build("confirmationCallback/4 array", function () {
  callbacksBuilder.confirmationCallback(
    "probe-440",
    1,
    ["left-442", "right-442"],
    0
  );
});
build("pollingWaitCallback/2", function () {
  callbacksBuilder.pollingWaitCallback("probe-450", "probe-451");
});
build("textOutputCallback/2", function () {
  callbacksBuilder.textOutputCallback(1, "probe-461");
});
build("validatedUsernameCallback/4", function () {
  callbacksBuilder.validatedUsernameCallback(
    "probe-470",
    { marker: "object-471" },
    true,
    ["left-473", "right-473"]
  );
});
build("validatedUsernameCallback/3", function () {
  callbacksBuilder.validatedUsernameCallback(
    "probe-480",
    { marker: "object-481" },
    true
  );
});
build("validatedPasswordCallback/5", function () {
  callbacksBuilder.validatedPasswordCallback(
    "probe-490",
    false,
    { marker: "object-492" },
    false,
    ["left-494", "right-494"]
  );
});
build("validatedPasswordCallback/4", function () {
  callbacksBuilder.validatedPasswordCallback(
    "probe-500",
    false,
    { marker: "object-502" },
    false
  );
});
outcome = "ok";
