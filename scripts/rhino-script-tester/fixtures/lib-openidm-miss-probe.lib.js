// Probes openidm.read miss behavior for the EXACT managed objects the IDR
// scorer reads: does a missing record throw a ResourceException, or return
// null? Determines whether the lookupNameVariant catch block fires on every
// normal miss (would over-log) or only on genuine read errors. Uploaded as
// LIBRARY script rhino-lib-openidm-miss-probe. Safe to delete.
function readMiss(path) {
  try {
    var rec = openidm.read(path);
    return {
      threw: false,
      value: rec === null ? "null" : rec === undefined ? "undefined" : "object",
    };
  } catch (e) {
    return { threw: true, error: String(e) };
  }
}

// Missing record inside an EXISTING managed object (the real name-variant path).
exports.variantMiss = readMiss(
  "managed/idr_name_variants/given__zzznotareal_zzzalsonotreal"
);
// Missing record inside the discrepancy telemetry object.
exports.discrepancyMiss = readMiss(
  "managed/idr_name_variant_discrepancies/zzznotareal"
);
// A managed object TYPE that does not exist at all (different failure mode).
exports.unknownTypeMiss = readMiss(
  "managed/zzz_no_such_object_type/zzznotareal"
);
