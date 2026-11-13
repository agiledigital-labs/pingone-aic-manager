// OIDC claims conformance probe: the `claims` binding, the logger's shape and
// returning a mutated `claims`.
var out = {};
var nextGen = typeof require === "function";
function clean(e) { return String(e).split(" in object ")[0].replace(/ \([^()]*#\d+\)$/, ""); }
function call(fn) { try { var v = fn(); return v === null ? "null" : v === undefined ? "undefined" : typeof v + "|" + String(v); } catch (e) { return "throw:" + clean(e); } }
out.subStrict = call(function () { return claims.sub === identity.getName(); });
out.subLoose = call(function () { return claims.sub == identity.getName(); });
out.stringify = call(function () { return JSON.stringify(claims); });
out.objectKeys = call(function () { return Object.keys(claims).join(","); });
out.hasOwn = call(function () { return Object.prototype.hasOwnProperty.call(claims, "sub"); });
out.inOp = call(function () { return "sub" in claims; });
out.missing = call(function () { return claims.zzNope; });
out.getMissing = call(function () { return claims.get("zzNope"); });
out.containsKey = call(function () { return claims.containsKey("sub"); });
out.putReturn = call(function () { return claims.put("zzPut", "p"); });
out.afterPut = call(function () { return claims.zzPut; });
out.loggerTypes = ["info", "warn", "error", "debug", "trace", "message", "warning", "errorEnabled", "messageEnabled",
  "warningEnabled", "isDebugEnabled", "getName"].map(function (n) { return n + "=" + typeof logger[n]; }).join(",");
out.loggerInfo = call(function () { logger.info("zz info {}", "x"); return "ok"; });
out.loggerMessage = call(function () { logger.message("zz message"); return "ok"; });
out.loggerWarningEnabled = call(function () { return logger.warningEnabled(); });
out.loggerError = call(function () { logger.error("zz error"); return "ok"; });
out.deleteSub = call(function () { return delete claims.subname; });
out.removeSub = call(function () { return claims.remove("subname"); });
out.afterRemove = call(function () { return claims.subname; });
var probe = JSON.stringify(out);
if (nextGen) {
  claims.aicprobe = probe;
  ({ values: claims, compositeScopes: {} });
} else {
  claims.put("aicprobe", probe);
  new org.forgerock.oauth2.core.UserInfoClaims(claims, new java.util.LinkedHashMap());
}
