// Probe: the key order of a next-gen openidm.query response, read three ways
// (for-in, Object.keys, and the order JSON.stringify writes) on the same
// response, for populated and empty queries with and without a field list.
// Two earlier probes disagreed: for-in over a populated query with no fields
// versus JSON.stringify of an empty query with fields. Reads only; emits key
// names, never record contents.
function emit(payload) {
  if (callbacks.isEmpty()) {
    callbacksBuilder.hiddenValueCallback("result", JSON.stringify(payload));
  }
  outcome = payload.ok ? "ok" : "error";
}
function attempt(f) { try { return f(); } catch (e) { return "err:" + String(e); } }
function orders(resp) {
  var forIn = [];
  for (var k in resp) { forIn.push(k); }
  return {
    forIn: forIn.join(),
    keys: attempt(function () { return Object.keys(resp).join(); }),
    json: attempt(function () { return Object.keys(JSON.parse(JSON.stringify(resp))).join(); })
  };
}
var out = { ok: true };
var P = "managed/alpha_user";
out.populatedNoFields = attempt(function () { return orders(openidm.query(P, { _queryFilter: "true", _pageSize: 2 })); });
out.populatedFields = attempt(function () { return orders(openidm.query(P, { _queryFilter: "true", _pageSize: 2 }, ["_id"])); });
out.emptyNoFields = attempt(function () { return orders(openidm.query(P, { _queryFilter: "false" })); });
out.emptyFields = attempt(function () { return orders(openidm.query(P, { _queryFilter: "false" }, ["_id"])); });
emit(out);
