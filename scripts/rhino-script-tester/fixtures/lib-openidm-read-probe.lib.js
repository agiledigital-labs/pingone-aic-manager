// Library body for the LIBRARY-context openidm.read probe. Uploaded as a
// LIBRARY script and required by lib-openidm-read-consumer.script.js. Probes
// whether a function inside a required library (not the top-level decision
// node script) can call openidm.read against a real managed object record.
// Reads a known seed record from managed/alpha_name_variant. Safe to delete.
function readVariant(id) {
  return openidm.read("managed/alpha_name_variant/" + id);
}
var rec = readVariant("aaron_erin");
exports.fromLib = rec ? { nameA: rec.nameA, nameB: rec.nameB, score: rec.score, imputed: rec.imputed } : null;

// miss behavior: does a nonexistent id throw, or return null/undefined?
var missResult;
try {
  var missRec = readVariant("zzznotarealtoken_zzzalsonotreal");
  missResult = { threw: false, value: missRec === null ? "null" : missRec === undefined ? "undefined" : JSON.stringify(missRec) };
} catch (e) {
  missResult = { threw: true, error: String(e) };
}
exports.missResult = missResult;
