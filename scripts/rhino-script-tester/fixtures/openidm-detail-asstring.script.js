// Probe: asString() and member presence checks on the JsonValue that
// e.javaException.getDetail() returns from a next-gen journey's openidm call,
// for a 403 policy failure (an object detail) and a 404 (a null detail).
// Creates nothing: the create is meant to fail, and is deleted at once if it
// does not.
function emit(payload) {
  if (callbacks.isEmpty()) {
    callbacksBuilder.hiddenValueCallback("result", JSON.stringify(payload));
  }
  outcome = payload.ok ? "ok" : "error";
}
function attempt(f) { try { return f(); } catch (e) { return "err:" + String(e); } }
function asStr(v) {
  return attempt(function () {
    var s = v.asString();
    return { typeof: typeof s, isNull: s === null, value: s === null ? null : String(s), eq: s === "frIndexedInteger1", length: s === null ? null : s.length };
  });
}
function presence(v) {
  var names = ["asString", "isString", "asBoolean", "asInteger", "asList", "asMap", "isNull", "get", "size", "keys", "expect", "defaultTo", "required", "class", "zzNotAMember"];
  var out = {};
  for (var i = 0; i < names.length; i++) {
    (function (n) {
      out[n] = { typeof: attempt(function () { return typeof v[n]; }), in: attempt(function () { return n in v; }) };
    })(names[i]);
  }
  return out;
}
function detailOf(f) {
  try { f(); return null; } catch (e) { return e.javaException.getDetail(); }
}
var id = "zzemuE-detail-" + utils.crypto.randomUUID();
var out = { ok: true, feature: "openidm-detail-asstring" };
var created = false;
var policy = detailOf(function () {
  var r = openidm.create("managed/bravo_user", id, { userName: id, givenName: "Zz", sn: "Emu", mail: id + "@example.com", frIndexedInteger1: null });
  created = true;
  return r;
});
if (created) { out.cleanup = attempt(function () { openidm.delete("managed/bravo_user/" + id, null); return "deleted"; }); }
var notFound = detailOf(function () { return openidm.delete("managed/alpha_user/" + id, null); });
out.policyNull = policy === null;
out.notFoundNull = notFound === null;
if (policy !== null && notFound !== null) {
  var fpr = policy.get("failedPolicyRequirements");
  var first = attempt(function () { return fpr.get(0); });
  var prop = attempt(function () { return first.get("property"); });
  out.getIndex = attempt(function () { return { typeof: typeof first, str: String(first) }; });
  out.property = attempt(function () { return { typeof: typeof prop, str: String(prop), isNull: prop.isNull() }; });
  out.asStringOnString = asStr(prop);
  out.asStringOnNullDetail = asStr(notFound);
  out.asStringOnNullFromNullDetail = asStr(notFound.get("result"));
  out.missing = attempt(function () { var m = policy.get("zzNoSuchKey"); return { typeof: typeof m, isNullJs: m === null, isNull: m.isNull(), str: String(m) }; });
  out.asStringOnMissing = asStr(policy.get("zzNoSuchKey"));
  out.asStringOnBoolean = asStr(policy.get("result"));
  out.asStringOnObject = asStr(policy);
  out.asStringOnArray = asStr(fpr);
  out.asStringWithArg = attempt(function () { return String(prop.asString(1)); });
  out.asStringValue = attempt(function () { return String(prop.asString); });
  out.asStringOnElement = asStr(first);
  out.asStringDeepArray = asStr(first.get("policyRequirements"));
  out.asStringDeepString = asStr(first.get("policyRequirements").get(0).get("policyRequirement"));
  out.getOutOfRange = attempt(function () { var v = fpr.get(5); return { typeof: typeof v, isNullJs: v === null, isNull: v.isNull(), str: String(v) }; });
  out.getNegative = attempt(function () { var v = fpr.get(-1); return { typeof: typeof v, isNullJs: v === null, isNull: v.isNull(), str: String(v) }; });
  out.getKeyOnArray = attempt(function () { var v = fpr.get("property"); return { typeof: typeof v, isNullJs: v === null, isNull: v.isNull(), str: String(v) }; });
  out.getIndexOnObject = attempt(function () { var v = policy.get(0); return { typeof: typeof v, isNullJs: v === null, isNull: v.isNull(), str: String(v) }; });
  out.getIndexOnString = attempt(function () { var v = prop.get(0); return { typeof: typeof v, str: String(v) }; });
  out.getKeyOnNull = attempt(function () { var v = notFound.get("x"); return { typeof: typeof v, isNull: v.isNull() }; });
  out.getIndexOnNull = attempt(function () { var v = notFound.get(0); return { typeof: typeof v, isNull: v.isNull() }; });
  out.presencePolicy = presence(policy);
  out.presenceNull = presence(notFound);
  out.presenceString = presence(prop);
}
emit(out);
