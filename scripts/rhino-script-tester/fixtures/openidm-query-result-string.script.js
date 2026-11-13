// Probe: how a next-gen openidm.query `result` converts — String(v),
// JSON.stringify(v), get(n) past the end of a populated list, and the
// missing-method error's full wording. Reads only. Emits shapes and the
// conversions of EMPTY lists verbatim; never a populated list's contents.
function emit(payload) {
  if (callbacks.isEmpty()) {
    callbacksBuilder.hiddenValueCallback("result", JSON.stringify(payload));
  }
  outcome = payload.ok ? "ok" : "error";
}
function attempt(f) { try { return f(); } catch (e) { return "err:" + String(e); } }
function shape(s) {
  if (typeof s !== "string") return typeof s;
  return {
    first: s.charAt(0), last: s.charAt(s.length - 1),
    hasBrace: s.indexOf("{") !== -1, hasEquals: s.indexOf("=") !== -1,
    hasQuoteColon: s.indexOf("\":") !== -1, hasIdKey: s.indexOf("_id") !== -1
  };
}
var out = { ok: true };
var full = openidm.query("managed/alpha_user", { _queryFilter: "true", _pageSize: 2 }, ["_id"]).result;
var empty = openidm.query("managed/alpha_user", { _queryFilter: "false" }, ["_id"]).result;
out.populated = {
  size: full.size(),
  typeofToJSON: typeof full.toJSON,
  string: shape(attempt(function () { return String(full); })),
  concatString: shape(attempt(function () { return "" + full; })),
  json: attempt(function () {
    var parsed = JSON.parse(JSON.stringify(full));
    return { isArray: Array.isArray(parsed), length: parsed.length, keys0: parsed[0] ? Object.keys(parsed[0]).sort().join() : null };
  }),
  getPastEnd: attempt(function () { return typeof full.get(full.size()); }),
  getNegative: attempt(function () { return typeof full.get(-1); }),
  someFull: attempt(function () { return full.some(function () { return true; }); }).split(" in object ")[0]
};
out.empty = {
  string: attempt(function () { return String(empty); }),
  json: attempt(function () { return JSON.stringify(empty); }),
  wrapperJson: attempt(function () {
    var q = openidm.query("managed/alpha_user", { _queryFilter: "false" }, ["_id"]);
    return JSON.stringify(q);
  }),
  some: attempt(function () { return empty.some(function () { return true; }); }),
  map: attempt(function () { return empty.map(function (x) { return x; }); })
};
emit(out);
