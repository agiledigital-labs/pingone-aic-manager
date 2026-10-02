// Probe: the populated fr-idm-custom-attrs bag, read and written through a
// next-gen ScriptedIdentity. Needs two temporary alpha_user properties,
// custom_rlProbe (string) and custom_rlProbeFlag (boolean), and a probe user
// rl-probe-custom seeded with custom_rlProbe "seeded" and custom_rlProbeFlag
// true. Each write case resets the record through openidm.patch first, then
// reports what IDM holds and what a fresh getIdentity reads back. Writes only
// the probe user.
function emit(payload) {
  if (callbacks.isEmpty()) {
    callbacksBuilder.hiddenValueCallback("result", JSON.stringify(payload));
  }
  outcome = payload.ok ? "ok" : "error";
}

var USER = "rl-probe-custom";
var FIELDS = ["custom_rlProbe", "custom_rlProbeFlag"];

function bag(identity) {
  var v = identity.getAttributeValues("fr-idm-custom-attrs");
  var arr = v.toArray();
  var items = [];
  for (var i = 0; i < arr.length; i++) {
    items.push(String(arr[i]));
  }
  return { size: v.size(), items: items };
}

function idmView(id) {
  var rec = openidm.read("managed/alpha_user/" + id, null, FIELDS);
  var out = {};
  for (var i = 0; i < FIELDS.length; i++) {
    out[FIELDS[i]] = rec[FIELDS[i]] === undefined ? "<absent>" : rec[FIELDS[i]];
  }
  return out;
}

function reset(id) {
  openidm.patch("managed/alpha_user/" + id, null, [
    { operation: "replace", field: "/custom_rlProbe", value: "seeded" },
    { operation: "replace", field: "/custom_rlProbeFlag", value: true },
  ]);
}

function writeCase(id, values) {
  var result = {};
  try {
    reset(id);
    var identity = idRepository.getIdentity(id);
    identity.setAttribute("fr-idm-custom-attrs", values);
    result.sameWrapperBeforeStore = bag(identity);
    identity.store();
    result.stored = true;
  } catch (e) {
    result.error = String(e);
  }
  try {
    result.idm = idmView(id);
    result.freshRead = bag(idRepository.getIdentity(id));
  } catch (e2) {
    result.readError = String(e2);
  }
  return result;
}

try {
  var out = {};
  // getIdentity resolves the fr-idm-uuid, not the userName.
  var idmId = openidm.query("managed/alpha_user", { _queryFilter: 'userName eq "' + USER + '"' }, ["_id"]).result[0]._id;
  reset(idmId);
  var identity = idRepository.getIdentity(idmId);
  out.read = bag(identity);
  out.directCustomName = identity.getAttributeValues("custom_rlProbe").size();
  out.writeOneKey = writeCase(idmId, [JSON.stringify({ custom_rlProbe: "written" })]);
  out.writeBothKeys = writeCase(idmId, [
    JSON.stringify({ custom_rlProbe: "both", custom_rlProbeFlag: false }),
  ]);
  out.writeFlagAsString = writeCase(idmId, [JSON.stringify({ custom_rlProbeFlag: "false" })]);
  out.writeUndeclaredKey = writeCase(idmId, [JSON.stringify({ custom_rlNope: "x" })]);
  out.writeEmptyObject = writeCase(idmId, [JSON.stringify({})]);
  out.writeEmptyList = writeCase(idmId, []);
  out.writeTwoElements = writeCase(idmId, [
    JSON.stringify({ custom_rlProbe: "a" }),
    JSON.stringify({ custom_rlProbeFlag: false }),
  ]);
  out.writeNotJson = writeCase(idmId, ["not json"]);
  reset(idmId);
  emit({ ok: true, feature: "identity-custom-attrs", value: JSON.stringify(out) });
} catch (e) {
  emit({ ok: false, feature: "identity-custom-attrs", error: String(e) });
}
