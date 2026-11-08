// Probe: what e.javaException.getDetail() returns from a next-gen journey's
// openidm call, for a 403 policy failure, a 404 and a 400. Creates nothing: the
// create is meant to fail, and is deleted at once if it does not.
function emit(payload) {
  if (callbacks.isEmpty()) {
    callbacksBuilder.hiddenValueCallback("result", JSON.stringify(payload));
  }
  outcome = payload.ok ? "ok" : "error";
}
function attempt(f) { try { return f(); } catch (e) { return "err:" + String(e); } }
function describeDetail(j) {
  var d;
  var got = attempt(function () { d = j.getDetail(); return "ok"; });
  if (got !== "ok") return { call: got };
  return {
    typeof: typeof d,
    isNull: d === null,
    isUndefined: d === undefined,
    str: attempt(function () { return String(d); }),
    json: attempt(function () { return JSON.stringify(d); }),
    typeofGet: attempt(function () { return typeof d.get; }),
    getResult: attempt(function () { return String(d.get("result")); }),
    jvIsNull: attempt(function () { return d.isNull(); }),
    getObject: attempt(function () { return JSON.stringify(d.getObject()); }),
    keys: attempt(function () { return Object.keys(d).join(","); }),
    forIn: attempt(function () { var k = []; for (var x in d) k.push(x); return k.join(","); }),
    dotResult: attempt(function () { return typeof d.result + ":" + String(d.result); }),
    dotFpr: attempt(function () { return typeof d.failedPolicyRequirements; })
  };
}
function caught(f) {
  try { var r = f(); return { threw: false, ret: attempt(function () { return JSON.stringify(r); }) }; }
  catch (e) {
    var j = e.javaException;
    return {
      threw: true,
      name: String(e.name),
      message: String(e.message),
      hasJ: typeof j,
      typeofGetDetail: attempt(function () { return typeof j.getDetail; }),
      inGetDetail: attempt(function () { return "getDetail" in j; }),
      code: attempt(function () { return j.getCode(); }),
      reason: attempt(function () { return String(j.getReason()); }),
      detail: describeDetail(j)
    };
  }
}
var id = "zzemu-detail-" + utils.crypto.randomUUID();
var out = { ok: true, feature: "openidm-error-detail" };
out.policy = caught(function () {
  return openidm.create("managed/bravo_user", id, { userName: id, givenName: "Zz", sn: "Emu", mail: id + "@example.com", frIndexedInteger1: null });
});
if (!out.policy.threw) { out.cleanup = attempt(function () { openidm.delete("managed/bravo_user/" + id, null); return "deleted"; }); }
out.notFound = caught(function () { return openidm.delete("managed/alpha_user/" + id, null); });
out.badAction = caught(function () { return openidm.action("managed/alpha_user", "zzemuUnknownAction", {}); });
emit(out);
