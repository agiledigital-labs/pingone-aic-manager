// Probe: do openidm.query's and openidm.read's `fields` arguments split a
// comma-joined element? Compares ["givenName","sn"] with ["givenName,sn"],
// ["givenName, sn"] and a mixed ["givenName,sn", "mail"] on one alpha_user
// row. Reports key NAMES only, never values. Read-only.
function emit(payload) {
  if (callbacks.isEmpty()) {
    callbacksBuilder.hiddenValueCallback("result", JSON.stringify(payload));
  }
  outcome = payload.ok ? "ok" : "error";
}

var PARAMS = { _queryFilter: "true", _pageSize: 1 };
var CASES = {
  separate: ["givenName", "sn"],
  joined: ["givenName,sn"],
  joinedSpace: ["givenName, sn"],
  mixed: ["givenName,sn", "mail"],
};

function keysOf(fields) {
  try {
    var result = openidm.query("managed/alpha_user", PARAMS, fields);
    var rows = result.result;
    if (!rows || rows.length === 0) {
      return "no-rows";
    }
    return Object.keys(rows[0]).sort().join(",");
  } catch (e) {
    return "error: " + String(e);
  }
}

function readKeysOf(id, fields) {
  try {
    return Object.keys(openidm.read("managed/alpha_user/" + id, null, fields)).sort().join(",");
  } catch (e) {
    return "error: " + String(e);
  }
}

try {
  var out = {};
  var name;
  for (name in CASES) {
    out["query." + name] = keysOf(CASES[name]);
  }
  var id = openidm.query("managed/alpha_user", PARAMS, ["_id"]).result[0]._id;
  for (name in CASES) {
    out["read." + name] = readKeysOf(id, CASES[name]);
  }
  emit({ ok: true, feature: "query-fields-comma", value: JSON.stringify(out) });
} catch (e) {
  emit({ ok: false, feature: "query-fields-comma", error: String(e) });
}
