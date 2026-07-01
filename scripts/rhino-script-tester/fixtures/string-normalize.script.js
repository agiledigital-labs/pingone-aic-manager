// Probe: String.prototype.normalize (NFD accent folding), node + library
// contexts. Pairs with lib-normalize-probe.lib.js (uploaded as LIBRARY script
// "rhino-lib-normalize-probe"). Each aspect probed independently so one
// failure does not mask the others. Safe to delete.
function emit(payload) {
  if (callbacks.isEmpty()) {
    callbacksBuilder.hiddenValueCallback("result", JSON.stringify(payload));
  }
  outcome = payload.ok ? "ok" : "error";
}

function probe(name, fn) {
  try {
    return { name: name, ok: true, value: String(fn()) };
  } catch (e) {
    return { name: name, ok: false, error: String(e) };
  }
}

try {
  var results = [];
  results.push(
    probe("normalize-exists", function () {
      return typeof "".normalize;
    })
  );
  results.push(
    probe("nfd-decompose", function () {
      // e-acute decomposes to base letter + combining mark
      return "é".normalize("NFD").length; // expect 2
    })
  );
  results.push(
    probe("nfd-fold-eacute", function () {
      return "José".normalize("NFD").replace(/[̀-ͯ]/g, ""); // expect Jose
    })
  );
  results.push(
    probe("nfd-fold-stacked", function () {
      // Vietnamese e with circumflex AND tilde: two combining marks
      return "Nguyễn".normalize("NFD").replace(/[̀-ͯ]/g, ""); // expect Nguyen
    })
  );
  results.push(
    probe("nfc-compose", function () {
      return ("é".normalize("NFC") === "é"); // expect true
    })
  );
  results.push(
    probe("lib-context", function () {
      var lib = require("rhino-lib-normalize-probe");
      return lib.foldedEacute + "|" + lib.foldedStacked + "|" + lib.nfcLength; // expect Jose|Nguyen|1
    })
  );
  var ok = results.every(function (r) { return r.ok; });
  emit({ ok: ok, feature: "string-normalize", results: results });
} catch (e) {
  emit({ ok: false, feature: "string-normalize", error: String(e) });
}
