// Probe: edge cases of a fr-idm-custom-attrs write that need no custom schema
// property. Uses a throwaway user rl-probe-bag-edges (no custom properties).
// Each case resets the bag to ["{}"] first, then reports the store() outcome
// and what a fresh getIdentity reads back. Writes only the probe user.
function emit(payload) {
  if (callbacks.isEmpty()) {
    callbacksBuilder.hiddenValueCallback("result", JSON.stringify(payload));
  }
  outcome = payload.ok ? "ok" : "error";
}

var USER = "rl-probe-bag-edges";

function bag(identity) {
  var v = identity.getAttributeValues("fr-idm-custom-attrs");
  var arr = v.toArray();
  var items = [];
  for (var i = 0; i < arr.length; i++) {
    items.push(String(arr[i]));
  }
  return { size: v.size(), items: items };
}

function setBag(id, values) {
  var identity = idRepository.getIdentity(id);
  identity.setAttribute("fr-idm-custom-attrs", values);
  identity.store();
}

function shortError(e) {
  var s = String(e);
  var m = s.match(/errorcode=\d+/);
  return m ? m[0] : s.substring(0, 200);
}

function writeCase(id, values) {
  var result = {};
  try {
    setBag(id, ["{}"]);
    setBag(id, values);
    result.stored = true;
  } catch (e) {
    result.error = shortError(e);
  }
  result.freshRead = bag(idRepository.getIdentity(id));
  // Which non-OOTB keys IDM reports on the record afterwards.
  try {
    var rec = openidm.read("managed/alpha_user/" + id);
    result.idmCustomKeys = Object.keys(rec)
      .filter(function (k) {
        return k.indexOf("custom_") === 0 || k === "plain";
      })
      .sort()
      .join(",");
  } catch (e) {
    result.idmReadError = shortError(e);
  }
  try {
    result.idmQueryRows = openidm.query("managed/alpha_user", { _queryFilter: 'userName eq "' + USER + '"' }, ["_id"]).result.length;
  } catch (e) {
    result.idmQueryError = shortError(e);
  }
  return result;
}

try {
  var out = {};
  var idmId = openidm.query("managed/alpha_user", { _queryFilter: 'userName eq "' + USER + '"' }, ["_id"]).result[0]._id;
  setBag(idmId, ["{}"]);
  out.initial = bag(idRepository.getIdentity(idmId));

  // Precedence: cardinality and syntax both wrong, in both orders.
  out.twoBadFirst = writeCase(idmId, ["not json", "{}"]);
  out.twoBadSecond = writeCase(idmId, ["{}", "not json"]);
  // JSON that is not an object.
  out.scalarString = writeCase(idmId, [JSON.stringify("x")]);
  out.scalarNumber = writeCase(idmId, ["1"]);
  out.jsonNull = writeCase(idmId, ["null"]);
  out.jsonArray = writeCase(idmId, ["[]"]);
  // An object key without the custom_ prefix.
  out.plainKey = writeCase(idmId, [JSON.stringify({ plain: "x" })]);
  out.nestedValue = writeCase(idmId, [JSON.stringify({ custom_rlNested: { a: 1 } })]);

  // Lifecycle: clear with [], then an IDM write of an ordinary property.
  setBag(idmId, ["{}"]);
  setBag(idmId, []);
  out.afterClear = bag(idRepository.getIdentity(idmId));
  openidm.patch("managed/alpha_user/" + idmId, null, [
    { operation: "replace", field: "/givenName", value: "Edges" + new Date().getTime() },
  ]);
  out.afterClearThenPatch = bag(idRepository.getIdentity(idmId));
  var rec = openidm.read("managed/alpha_user/" + idmId);
  out.afterClearThenUpdate = (function () {
    try {
      openidm.update("managed/alpha_user/" + idmId, null, rec);
      return bag(idRepository.getIdentity(idmId));
    } catch (e) {
      return { error: shortError(e) };
    }
  })();

  setBag(idmId, ["{}"]);
  emit({ ok: true, feature: "identity-custom-attrs-edges", value: JSON.stringify(out) });
} catch (e) {
  emit({ ok: false, feature: "identity-custom-attrs-edges", error: String(e) });
}
