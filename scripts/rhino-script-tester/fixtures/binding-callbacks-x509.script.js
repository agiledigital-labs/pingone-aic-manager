// Probe: callbacksBuilder.x509CertificateCallback, all overloads. Safe to delete.
// AM cannot serialise an X509CertificateCallback to REST JSON; measured
// 2026-09-29 as a 400 for the whole /authenticate response.
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
build("x509CertificateCallback/2", function () {
  callbacksBuilder.x509CertificateCallback("probe-230", {
    marker: "object-231",
  });
});
build("x509CertificateCallback/1", function () {
  callbacksBuilder.x509CertificateCallback("probe-240");
});
build("x509CertificateCallback/3", function () {
  callbacksBuilder.x509CertificateCallback(
    "probe-250",
    { marker: "object-251" },
    true
  );
});
outcome = "ok";
