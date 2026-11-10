// OIDC claims conformance probe: the request's `claims` and `claims_locales`
// parameters as the script sees them.
var out = {};
var nextGen = typeof require === "function";
function clean(e) { return String(e).split(" in object ")[0].replace(/ \([^()]*#\d+\)$/, ""); }
function shape(v) {
  if (v === null) return "null";
  if (v === undefined) return "undefined";
  var size = "n/a";
  if (typeof v === "object" && typeof v.size === "function") size = String(v.size());
  var len = typeof v.length === "function" ? "function" : String(v.length);
  return typeof v + "|" + String(v) + "|size=" + size + "|len=" + len;
}
function call(fn) { try { return shape(fn()); } catch (e) { return "throw:" + clean(e); } }
function keys(fn) { try { var o = fn(); var k = []; for (var x in o) k.push(x); k.sort(); return k.join(","); } catch (e) { return "throw:" + clean(e); } }
out.where = call(function () { return String(requestProperties.requestUri).replace(/^https?:\/\/[^\/]+/, "").split("/").pop(); });
out.requestedClaims = call(function () { return requestedClaims; });
out.requestedKeys = keys(function () { return requestedClaims; });
out.requestedReq = call(function () { return requestedClaims.zzReq; });
out.requestedReqGet = call(function () { return requestedClaims.get("zzReq"); });
out.requestedVal = call(function () { return requestedClaims.zzVal; });
out.requestedVal0 = call(function () { return requestedClaims.zzVal[0]; });
out.requestedValArr = call(function () { return requestedClaims.zzVal.toArray()[0]; });
out.claimsLocales = call(function () { return claimsLocales; });
out.claimsLocales0 = call(function () { return claimsLocales[0]; });
out.claimsLocales0Type = call(function () { return typeof claimsLocales[0]; });
out.claims = call(function () { return claims; });
out.claimsKeys = keys(function () { return claims; });
out.claimObjects = call(function () { return claimObjects; });
out.requestedTypedClaims = call(function () { return requestedTypedClaims; });
function claimRow(c) {
  return [call(function () { return c.getName(); }), call(function () { return c.getValues(); }),
    call(function () { return c.isEssential(); }), call(function () { return c.getLocale(); }),
    call(function () { return c.getNameWithLocale(); })].join(" ; ");
}
out.typed = call(function () { var a = requestedTypedClaims.toArray(); var r = []; for (var i = 0; i < a.length; i++) r.push(claimRow(a[i])); return r.join(" || "); });
out.objects = call(function () { var a = claimObjects.toArray(); var r = []; for (var i = 0; i < a.length; i++) r.push(claimRow(a[i])); return r.join(" || "); });
var probe = JSON.stringify(out);
if (nextGen) {
  ({ values: { aicprobe: probe }, compositeScopes: {} });
} else {
  var m = new java.util.LinkedHashMap();
  m.put("aicprobe", probe);
  new org.forgerock.oauth2.core.UserInfoClaims(m, new java.util.LinkedHashMap());
}
