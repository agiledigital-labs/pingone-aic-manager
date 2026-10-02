// Probe: IDM access to a user whose fr-idm-custom-attrs bag is JSON but not an
// object. Uses a throwaway user rl-probe-bag-nonobject (no custom properties).
// Reports a full read after a boolean bag, then deletes the user while it
// carries a string bag. That delete fails (measured 2026-10-02, and REST
// DELETE answers 500 too), so the probe then resets the bag to {} through AM
// and deletes the user again. Writes only the probe user, and deletes it.
function emit(payload) {
  if (callbacks.isEmpty()) {
    callbacksBuilder.hiddenValueCallback("result", JSON.stringify(payload));
  }
  outcome = payload.ok ? "ok" : "error";
}

var USER = "rl-probe-bag-nonobject";

function setBag(id, values) {
  var identity = idRepository.getIdentity(id);
  identity.setAttribute("fr-idm-custom-attrs", values);
  identity.store();
}

function attempt(fn) {
  try {
    var value = fn();
    return { ok: true, value: value };
  } catch (e) {
    return { ok: false, error: String(e).substring(0, 240) };
  }
}

try {
  var out = {};
  var path = "managed/alpha_user/";
  var id = openidm.query("managed/alpha_user", { _queryFilter: 'userName eq "' + USER + '"' }, ["_id"]).result[0]._id;

  setBag(id, ["true"]);
  out.booleanFullRead = attempt(function () {
    return Object.keys(openidm.read(path + id)).length;
  });
  out.booleanAmRead = attempt(function () {
    return String(idRepository.getIdentity(id).getAttributeValues("fr-idm-custom-attrs").toArray()[0]);
  });

  setBag(id, [JSON.stringify("x")]);
  out.stringDelete = attempt(function () {
    var deleted = openidm.delete(path + id, null);
    return deleted === null ? "null" : Object.keys(deleted).sort().join(",");
  });
  out.afterDeleteRows = attempt(function () {
    return openidm.query("managed/alpha_user", { _queryFilter: 'userName eq "' + USER + '"' }, ["_id"]).result.length;
  });
  setBag(id, ["{}"]);
  out.deleteAfterReset = attempt(function () {
    openidm.delete(path + id, null);
    return openidm.query("managed/alpha_user", { _queryFilter: 'userName eq "' + USER + '"' }, ["_id"]).result.length;
  });
  emit({ ok: true, feature: "identity-custom-attrs-nonobject", value: JSON.stringify(out) });
} catch (e) {
  emit({ ok: false, feature: "identity-custom-attrs-nonobject", error: String(e) });
}
