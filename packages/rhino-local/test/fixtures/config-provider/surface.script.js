// Config provider node probe: the binding surface. Runs unchanged on AIC (both
// engines) and in the emulator. Never serialises requestHeaders,
// requestParameters, requestCookies or existingSession, and never reads a token.
var out = {};
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
function count(fn) { try { var o = fn(); var n = 0; for (var x in o) n++; return n; } catch (e) { return "throw:" + clean(e); } }
var names = ["config", "nodeState", "sharedState", "transientState", "callbacks", "callbacksBuilder", "action",
  "outcome", "idRepository", "existingSession", "requestHeaders", "requestParameters", "requestCookies", "realm",
  "scriptName", "logger", "httpClient", "secrets", "systemEnv", "openidm", "utils", "emailService", "jwtAssertion",
  "jwtValidator", "policy", "cookieName", "journey", "oauthApplication", "samlApplication", "locales",
  "resumedFromSuspend", "require", "identity", "session", "auditEntryDetail", "properties", "JavaImporter",
  "Packages", "context", "org", "java"];
out.typeof = {};
for (var i = 0; i < names.length; i++) {
  try { out.typeof[names[i]] = eval("typeof " + names[i]); } catch (e) { out.typeof[names[i]] = "throw:" + clean(e); }
}
out.configBefore = call(function () { return config; });
out.nsStr = call(function () { return nodeState.get("zzStr"); });
out.nsStrStrict = call(function () { return nodeState.get("zzStr") === "hello"; });
out.nsNum = call(function () { return nodeState.get("zzNum"); });
out.nsNumType = call(function () { return typeof nodeState.get("zzNum"); });
out.nsObj = call(function () { return nodeState.get("zzObj"); });
out.nsObjJson = call(function () { return JSON.stringify(nodeState.get("zzObj")); });
out.nsObjA = call(function () { return nodeState.get("zzObj").a; });
out.nsBool = call(function () { return nodeState.get("zzBool"); });
out.nsTransient = call(function () { return nodeState.get("zzT"); });
out.nsMissing = call(function () { return nodeState.get("zzNope"); });
out.nsKeys = call(function () { var k = nodeState.keys(); var a = []; var it = k.iterator(); while (it.hasNext()) a.push(String(it.next())); a.sort(); return a.join(","); });
out.nsIsDefined = call(function () { return nodeState.isDefined("zzStr"); });
out.nsGetObject = call(function () { return typeof nodeState.getObject; });
out.nsPutShared = call(function () { nodeState.putShared("zzCfgPut", "p"); return "ok"; });
out.nsPutTransient = call(function () { nodeState.putTransient("zzCfgT", "t"); return "ok"; });
out.ssStr = call(function () { return sharedState.get("zzStr"); });
out.tsT = call(function () { return transientState.get("zzT"); });
out.realm = call(function () { return realm; });
out.scriptName = call(function () { return scriptName; });
out.loggerName = call(function () { return logger.getName(); });
out.reqParamCount = count(function () { return requestParameters; });
out.reqParamZzq = call(function () { return requestParameters.get("zzq"); });
out.reqHeaderCount = count(function () { return requestHeaders; });
out.reqHeaderProbe = call(function () { return requestHeaders.get("x-zzemu-probe"); });
out.cookieCount = count(function () { return requestCookies; });
out.existingSession = call(function () { return existingSession === null ? null : typeof existingSession; });
out.callbacksEmpty = call(function () { return callbacks.isEmpty(); });
out.resumed = call(function () { return resumedFromSuspend; });
out.journeyKeys = keys(function () { return journey; });
out.locales = call(function () { return typeof locales; });
out.idRepoKeys = keys(function () { return idRepository; });
var probe = JSON.stringify(out);
try { nodeState.putShared("aicprobe", probe); } catch (e) { /* putShared may be absent */ }
config = { message: { en: probe } };
