// Probe: is openidm.query(...).result a JS array or a Java list, on a next-gen
// scripted decision? And a has-many relationship array on a read? Reads only;
// emits types and counts, never record contents or ids.
function emit(payload) {
  if (callbacks.isEmpty()) {
    callbacksBuilder.hiddenValueCallback("result", JSON.stringify(payload));
  }
  outcome = payload.ok ? "ok" : "error";
}
function attempt(f) { try { return f(); } catch (e) { return "err:" + String(e); } }
// An error whose message would print the list's records: name and prefix only.
function attemptQuiet(f) { try { return f(); } catch (e) { return "err:" + String(e.name) + ": " + String(e.message).split(" in object ")[0]; } }
function describe(v) {
  return {
    typeof: typeof v,
    isArray: attempt(function () { return Array.isArray(v); }),
    length: attempt(function () { return v.length; }),
    index0Type: attempt(function () { return typeof v[0]; }),
    size: attempt(function () { return v.size(); }),
    get0Type: attempt(function () { return typeof v.get(0); }),
    typeofSome: attempt(function () { return typeof v.some; }),
    typeofMap: attempt(function () { return typeof v.map; }),
    typeofForEach: attempt(function () { return typeof v.forEach; }),
    typeofFilter: attempt(function () { return typeof v.filter; }),
    typeofJoin: attempt(function () { return typeof v.join; }),
    some: attemptQuiet(function () { return v.some(function () { return true; }); }),
    concatIsArray: attempt(function () { return Array.isArray([].concat(v)); }),
    concatLength: attempt(function () { return [].concat(v).length; }),
    sliceCall: attempt(function () { return Array.prototype.slice.call(v).length; }),
    forInKeys: attempt(function () { var n = 0; for (var k in v) n++; return n; }),
    getClass: attempt(function () { return String(v.getClass().getName()); })
  };
}
var out = { ok: true };
var q = attempt(function () { return openidm.query("managed/alpha_user", { _queryFilter: "true", _pageSize: 2 }, ["_id"]); });
if (typeof q === "string") {
  out.query = q;
} else {
  out.query = describe(q.result);
  out.emptyQuery = describe(openidm.query("managed/alpha_user", { _queryFilter: "false" }, ["_id"]).result);
  var id = attempt(function () { return String(q.result[0]._id); });
  if (id.indexOf("err:") !== 0) {
    var rec = attempt(function () { return openidm.read("managed/alpha_user/" + id, null, ["roles"]); });
    out.readRoles = typeof rec === "string" ? rec : describe(rec.roles);
  }
}
emit(out);
