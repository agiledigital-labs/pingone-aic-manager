// Library body probing the two ES2015 array-construction helpers the IDR
// scorer wants to use, inside a require()d LIBRARY script (not just the
// decision-node top level):
//   1. new Array(n).fill(false)              -- Array.prototype.fill
//   2. Array.from({ length: n }, () => false) -- Array.from + arrow mapper
// Each probe self-reports {ok, length, joined, allFalse} or {ok:false, error}
// so a missing method (TypeError) and a silent-wrong result are both
// distinguishable. Uploaded as LIBRARY script rhino-lib-array-fill-probe.
// Safe to delete.
function probe(build) {
  try {
    var a = build(3);
    var joined = a.join(",");
    return {
      ok: true,
      length: a.length,
      joined: joined,
      allFalse: joined === "false,false,false",
    };
  } catch (e) {
    return { ok: false, error: String(e) };
  }
}

exports.fill = probe(function (n) {
  return new Array(n).fill(false);
});

exports.from = probe(function (n) {
  return Array.from({ length: n }, () => false);
});
