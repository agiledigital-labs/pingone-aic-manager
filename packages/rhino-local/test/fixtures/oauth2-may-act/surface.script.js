// May-act surface probe (next-gen). The record leaves as the may_act
// claim's "probe" member through token.setMayAct(object). Shapes and call
// outcomes only. Never renders the token itself: only its
// length class, prefix and dot count. Never calls identity.getAttributes().
var out = {};
function clean(s) {
  return String(s).split(" in object ")[0].replace(/https?:\/\/[^\/\s"]+/g, "<host>").replace(/eyJ[A-Za-z0-9_\-.]+/g, "<jwt>").slice(0, 160);
}
function shape(v) {
  if (v === null) return "null";
  if (v === undefined) return "undefined";
  var t = typeof v;
  var tag = Object.prototype.toString.call(v);
  var s;
  try { s = clean(String(v)); } catch (e) { s = "String-throw:" + clean(e); }
  var size = "n/a";
  try { if (typeof v.size === "function") size = v.size(); } catch (e2) { size = "throw"; }
  var len;
  try { len = v.length; } catch (e3) { len = "throw"; }
  return { t: t, tag: tag, isArray: Array.isArray(v), size: String(size), len: String(len), str: s.length > 100 ? s.slice(0, 100) + "…" : s };
}
function call(fn) {
  try { return { ok: shape(fn()) }; } catch (e) { return { err: clean(e) }; }
}
var names = ["token", "accessToken", "scopes", "identity", "requestProperties", "clientProperties", "logger", "httpClient",
  "systemEnv", "realm", "scriptName", "cookieName", "secrets", "openidm", "utils", "emailService", "jwtAssertion",
  "jwtValidator", "policy", "require", "session", "nodeState", "sharedState", "transientState", "callbacks", "action",
  "idRepository", "requestHeaders", "requestParameters", "existingSession", "cacheManager", "outcome", "claims",
  "requestedScopes", "scopeValidatorHelper", "JavaImporter", "java", "Packages", "context", "properties"];
var o = out;
o.typeof = {};
for (var i = 0; i < names.length; i++) {
  var tv;
  try { tv = eval("typeof " + names[i]); } catch (e) { tv = "throw:" + clean(e); }
  o.typeof[names[i]] = tv;
}
o.tokenT = call(function () { return typeof token + "|" + Object.prototype.toString.call(token); });
o.tokenStr = call(function () { var s = String(token); return s.length + "|" + s.slice(0, 3) + "|" + s.split(".").length; });
try { var tk = []; for (var x in token) tk.push(x + ":" + typeof token[x]); tk.sort(); o.tokenMembers = tk; } catch (e5) { o.tokenMembers = "throw:" + clean(e5); }
var tm = ["getField", "getAct", "getMayAct", "setAct", "setMayAct", "setField", "getScope", "setScope", "getClientId",
  "getResourceOwnerId", "getTokenId", "getExpiryTime", "toString", "getClass", "equals", "hashCode", "addExtraData",
  "getRealm", "getTokenName", "getTokenType", "getAuditTrackingId", "getPermissions", "getConfirmationKey", "removeField"];
o.tokenTypeof = {};
for (var j = 0; j < tm.length; j++) {
  try { o.tokenTypeof[tm[j]] = typeof token[tm[j]]; } catch (e4) { o.tokenTypeof[tm[j]] = "throw:" + clean(e4); }
}
o.fClient = call(function () { var v = token.getField("client_id"); return typeof v + "|" + Object.prototype.toString.call(v) + "|" + v + "|" + (v === "zzemuC6-ma"); });
o.fSub = call(function () { return token.getField("sub"); });
o.fScope = call(function () { return token.getField("scope"); });
o.fScope0 = call(function () { var v = token.getField("scope"); return typeof v[0] + "|" + v[0]; });
o.fGrant = call(function () { return token.getField("grant_type"); });
o.fExp = call(function () { var v = token.getField("expires_in"); return typeof v + "|" + v; });
o.fRealm = call(function () { return token.getField("realm"); });
o.fMayAct = call(function () { return token.getField("may_act"); });
o.fAbsent = call(function () { return token.getField("zzabsent"); });
o.fAbsentNull = call(function () { return token.getField("zzabsent") === null; });
o.fNoArg = call(function () { return token.getField(); });
o.fNumber = call(function () { return token.getField(7); });
o.getAct0 = call(function () { return token.getAct(); });
o.getAct0Null = call(function () { return token.getAct() === null; });
o.getMayAct0 = call(function () { return token.getMayAct(); });
o.getMayAct0Null = call(function () { return token.getMayAct() === null; });
o.scopes = call(function () { return scopes; });
o.scopes0 = call(function () { return typeof scopes[0] + "|" + scopes[0]; });
o.scopesContains = call(function () { return scopes.contains("zzemuc6-a"); });
o.scopesToArray = call(function () { var a = scopes.toArray(); return typeof a + "|" + Array.isArray(a) + "|" + a.length + "|" + typeof a[0]; });
o.scopesJson = call(function () { return JSON.stringify(scopes); });
o.scopesForEach = call(function () { var a = []; scopes.forEach(function (x) { a.push(typeof x + ":" + x); }); return a.join(","); });
o.scopesIter = call(function () { return scopes.iterator(); });
o.identity = call(function () { return identity; });
o.idExists = call(function () { return identity.exists(); });
o.idName = call(function () { return identity.getName(); });
o.rp = call(function () { return typeof requestProperties + "|" + Object.prototype.toString.call(requestProperties) + "|" + requestProperties.size(); });
o.rpKeys = call(function () { var k = []; for (var x in requestProperties) k.push(x); k.sort(); return k.join(","); });
o.rpParamKeys = call(function () { var k = []; for (var x in requestProperties.requestParams) k.push(x); k.sort(); return k.join(","); });
o.rpGrant0 = call(function () { var g = requestProperties.requestParams.grant_type[0]; return typeof g + "|" + g + "|" + (g === "client_credentials"); });
o.rpGrantGet = call(function () { return requestProperties.requestParams.get("grant_type"); });
o.rpGetMissing = call(function () { return requestProperties.requestParams.get("zzmissing"); });
o.rpHeaderCt = call(function () { return requestProperties.requestHeaders["content-type"]; });
o.rpRealm = call(function () { return requestProperties.realm; });
o.rpUriT = call(function () { return typeof requestProperties.requestUri + "|" + clean(requestProperties.requestUri); });
o.cp = call(function () { return typeof clientProperties + "|" + Object.prototype.toString.call(clientProperties) + "|" + clientProperties.size(); });
o.cpKeys = call(function () { var k = []; for (var x in clientProperties) k.push(x); k.sort(); return k.join(","); });
o.cpId = call(function () { var c = clientProperties.clientId; return typeof c + "|" + c; });
o.cpGrants = call(function () { return clientProperties.allowedGrantTypes; });
o.cpRedirect = call(function () { return clientProperties.redirectUris; });
o.cpCustom = call(function () { return clientProperties.customProperties; });
o.realm = call(function () { return realm; });
o.scriptName = call(function () { return scriptName; });
o.cookieNameT = call(function () { return typeof cookieName; });
o.loggerName = call(function () { return logger.getName(); });
o.thisT = call(function () { return typeof this; });
// Set act first, read both back, then stamp the record.
o.setAct = call(function () { return token.setAct({ sub: "zzemuC6-actor" }); });
o.getAct1 = call(function () { return token.getAct(); });
o.getAct1Sub = call(function () { var a = token.getAct(); return typeof a.sub + "|" + a.sub + "|" + (typeof a.get === "function" ? a.get("sub") : "noget"); });
o.setMayAct1 = call(function () { return token.setMayAct({ client_id: "zzemuC6-first" }); });
o.getMayAct1 = call(function () { return token.getMayAct(); });
o.getMayAct1Json = call(function () { return JSON.stringify(token.getMayAct()); });
o.getMayAct1Id = call(function () { var m = token.getMayAct(); return typeof m.client_id + "|" + m.client_id; });
o.fMayAct1 = call(function () { return token.getField("may_act"); });
o.fAct1 = call(function () { return token.getField("act"); });
var rec = JSON.stringify(out);
if (typeof org !== "undefined" && typeof org.forgerock !== "undefined" && typeof require === "undefined") {
  var m = new java.util.LinkedHashMap();
  m.put("client_id", "zzemuC6-x");
  m.put("probe", rec);
  token.setMayAct(new org.forgerock.json.JsonValue(m));
} else {
  token.setMayAct({ client_id: "zzemuC6-x", probe: rec });
}
