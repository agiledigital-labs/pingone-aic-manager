// Social IdP profile transformation probe: the binding surface. Runs unchanged
// on AIC (both engines) and in the emulator. Never serialises requestHeaders,
// requestParameters or existingSession, and never reads a token value.
var out = {};
var nextGen = typeof require === "function";
function clean(e) { return String(e).split(" in object ")[0].replace(/ \([^()]*#\d+\)$/, ""); }
function tag(v) { return Object.prototype.toString.call(v); }
function shape(v) {
  if (v === null) return "null";
  if (v === undefined) return "undefined";
  var len = "n/a";
  try { len = typeof v.length === "function" ? "function" : String(v.length); } catch (e) { len = "throw"; }
  return typeof v + "|" + tag(v) + "|" + String(v) + "|len=" + len;
}
function call(fn) { try { return shape(fn()); } catch (e) { return "throw:" + clean(e); } }
function keys(fn) { try { var o = fn(); var k = []; for (var x in o) k.push(x); k.sort(); return k.join(","); } catch (e) { return "throw:" + clean(e); } }
var names = ["rawProfile", "normalizedProfile", "selectedIdp", "nodeState", "sharedState", "transientState", "logger",
  "httpClient", "requestParameters", "requestHeaders", "existingSession", "realm", "scriptName", "cookieName",
  "secrets", "systemEnv", "openidm", "utils", "emailService", "jwtAssertion", "jwtValidator", "policy", "require",
  "callbacks", "callbacksBuilder", "action", "outcome", "identity", "session", "idRepository", "accessToken",
  "claims", "scopes", "JavaImporter", "Packages", "context", "inputData", "profile", "json", "object", "field",
  "org", "java"];
out.typeof = {};
for (var i = 0; i < names.length; i++) {
  try { out.typeof[names[i]] = eval("typeof " + names[i]); } catch (e) { out.typeof[names[i]] = "throw:" + clean(e); }
}
out.raw = call(function () { return rawProfile; });
out.rawKeys = keys(function () { return rawProfile; });
out.rawSub = call(function () { return rawProfile.sub; });
out.rawSubType = call(function () { return typeof rawProfile.sub; });
out.rawGet = call(function () { return rawProfile.get("sub"); });
out.rawGetObject = call(function () { return rawProfile.get("sub").getObject(); });
out.rawIsDefined = call(function () { return rawProfile.isDefined("sub"); });
out.rawMissing = call(function () { return rawProfile.zzNope; });
out.rawGetMissing = call(function () { return rawProfile.get("zzNope"); });
out.rawJson = call(function () { return JSON.stringify(rawProfile); });
out.rawObject = call(function () { return rawProfile.getObject(); });
out.rawKeySet = call(function () { return rawProfile.keys(); });
out.rawSize = call(function () { return rawProfile.size(); });
out.idp = call(function () { return selectedIdp; });
out.idpStrict = call(function () { return selectedIdp === "zzemuC3-idp"; });
out.realm = call(function () { return realm; });
out.scriptName = call(function () { return scriptName; });
out.cookieName = call(function () { return typeof cookieName; });
out.reqParamKeys = call(function () { var n = 0; for (var x in requestParameters) n++; return n; });
out.reqHeaderKeys = call(function () { var n = 0; for (var x in requestHeaders) n++; return n; });
out.existingSession = call(function () { return existingSession === null ? null : typeof existingSession; });
out.nsSelected = call(function () { return nodeState.get("selectedIdp"); });
out.nsPut = call(function () { nodeState.putShared("zzemuC3PutShared", "s"); return "ok"; });
out.nsKeys = call(function () { var k = nodeState.keys(); var a = []; var it = k.iterator(); while (it.hasNext()) a.push(String(it.next())); a.sort(); return a.join(","); });
out.loggerName = call(function () { return logger.getName(); });
var probe = JSON.stringify(out);
if (nextGen) nodeState.putShared("aicprobe", probe);
({ id: "zz-id", displayName: "zz display", givenName: "zzGiven", familyName: "zzFamily", email: "zz@example.com",
  username: "zzuser", aicprobe: probe });
