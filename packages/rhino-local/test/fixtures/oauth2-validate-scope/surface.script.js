// Validate-scope surface probe (next-gen). The record is the denial
// message, through scopeValidatorHelper.throwInvalidScope. Shapes and call outcomes only.
// Never calls identity.getAttributes(); never prints header or parameter
// values other than grant_type and scope.
var out = { top: {}, fn: {} };
function clean(s) {
  return String(s).split(" in object ")[0].replace(/https?:\/\/[^\/\s"]+/g, "<host>").slice(0, 160);
}
function shape(v) {
  if (v === null) return "null";
  if (v === undefined) return "undefined";
  var t = typeof v;
  var tag = Object.prototype.toString.call(v);
  var s;
  try { s = String(v); } catch (e) { s = "String-throw:" + clean(e); }
  var size = "n/a";
  try { if (typeof v.size === "function") size = v.size(); } catch (e2) { size = "throw"; }
  var len;
  try { len = v.length; } catch (e3) { len = "throw"; }
  return { t: t, tag: tag, isArray: Array.isArray(v), size: String(size), len: String(len), str: s.length > 100 ? s.slice(0, 100) + "…" : s };
}
function call(fn) {
  try { return { ok: shape(fn()) }; } catch (e) { return { err: clean(e) }; }
}
function list(name, v) {
  var r = {};
  r.shape = call(function () { return v; });
  r.i0 = call(function () { return v[0]; });
  r.t0 = call(function () { return typeof v[0]; });
  r.eq0 = call(function () { return v[0] === "zzemuc5-a" || v[0] === "zzemuc5-b"; });
  r.contains = call(function () { return v.contains("zzemuc5-a"); });
  r.indexOf = call(function () { return v.indexOf("zzemuc5-a"); });
  r.join = call(function () { return v.join(","); });
  r.forEach = call(function () { var a = []; v.forEach(function (x) { a.push(typeof x + ":" + x); }); return a.join(","); });
  r.toArray = call(function () { var a = v.toArray(); return typeof a + "|" + Array.isArray(a) + "|" + a.length + "|" + typeof a[0]; });
  r.slice = call(function () { var a = Array.prototype.slice.call(v); return Array.isArray(a) + "|" + a.length + "|" + typeof a[0]; });
  r.json = call(function () { return JSON.stringify(v); });
  r.forIn = call(function () { var k = []; for (var x in v) k.push(x); return k.join(","); });
  r.isEmpty = call(function () { return v.isEmpty(); });
  r.add = call(function () { return v.add("zzemuc5-x"); });
  r.afterAdd = call(function () { return String(v) + "|" + v.length; });
  r.iterator = call(function () { return v.iterator(); });
  r.concat = call(function () { return [].concat(v).length; });
  return r;
}
var names = ["scopeValidatorHelper", "requestedScopes", "availableScopes", "allowedScopes", "defaultScopes", "identity",
  "requestProperties", "clientProperties", "logger", "httpClient", "systemEnv", "realm", "scriptName", "cookieName",
  "secrets", "openidm", "utils", "emailService", "jwtAssertion", "jwtValidator", "policy", "require", "accessToken",
  "token", "scopes", "session", "nodeState", "sharedState", "transientState", "callbacks", "action", "idRepository",
  "requestHeaders", "requestParameters", "existingSession", "cacheManager", "outcome", "claims", "JavaImporter",
  "java", "Packages", "validateAccessTokenScope", "validateRefreshTokenScope", "validateAuthorizationScope"];
function typeofs() {
  var o = {};
  for (var i = 0; i < names.length; i++) {
    var tv;
    try { tv = eval("typeof " + names[i]); } catch (e) { tv = "throw:" + clean(e); }
    o[names[i]] = tv;
  }
  return o;
}
out.top.typeof = typeofs();
out.top.reqStr = call(function () { return String(requestedScopes); });
function body(o) {
  o.typeof = typeofs();
  o.requested = list("requestedScopes", requestedScopes);
  o.available = call(function () { return availableScopes; });
  o.availableSorted = call(function () { var a = []; for (var i = 0; i < availableScopes.length; i++) a.push(String(availableScopes[i])); return a.sort().join(","); });
  o.allowed = call(function () { return allowedScopes; });
  o.defaults = call(function () { return defaultScopes; });
  o.defaults0 = call(function () { return typeof defaultScopes[0]; });
  o.avail0 = call(function () { return typeof availableScopes[0]; });
  o.allowedList = list("allowedScopes", allowedScopes);
  o.helper = call(function () { return scopeValidatorHelper; });
  o.helperMembers = call(function () { var k = []; for (var x in scopeValidatorHelper) k.push(x + ":" + typeof scopeValidatorHelper[x]); k.sort(); return k.join(","); });
  o.helperInherit0 = call(function () { return scopeValidatorHelper.inheritAccessTokenScopesOnRefresh(); });
  o.helperToString = call(function () { return scopeValidatorHelper.toString(); });
  o.identity = call(function () { return identity; });
  o.idName = call(function () { return identity.getName(); });
  o.idExists = call(function () { return identity.exists(); });
  o.rp = call(function () { return typeof requestProperties + "|" + Object.prototype.toString.call(requestProperties) + "|" + requestProperties.size(); });
  o.rpKeys = call(function () { var k = []; for (var x in requestProperties) k.push(x); k.sort(); return k.join(","); });
  o.rpRealm = call(function () { return requestProperties.realm; });
  o.rpRealmT = call(function () { return typeof requestProperties.realm + "|" + (requestProperties.realm === "/alpha"); });
  o.rpParamKeys = call(function () { var k = []; for (var x in requestProperties.requestParams) k.push(x); k.sort(); return k.join(","); });
  o.rpGrant0 = call(function () { var g = requestProperties.requestParams.grant_type[0]; return typeof g + "|" + g + "|" + (g === "client_credentials"); });
  o.rpGrant = call(function () { return requestProperties.requestParams.grant_type; });
  o.rpGrantGet = call(function () { return requestProperties.requestParams.get("grant_type"); });
  o.rpGetMissing = call(function () { return requestProperties.requestParams.get("zzmissing"); });
  o.rpMissing = call(function () { return requestProperties.requestParams.zzmissing; });
  o.rpScope = call(function () { return requestProperties.requestParams.scope; });
  o.rpHeaderCt = call(function () { return requestProperties.requestHeaders["content-type"]; });
  o.rpHeaderCtUpper = call(function () { return requestProperties.requestHeaders["Content-Type"]; });
  o.rpHeaderCount = call(function () { var n = 0; for (var x in requestProperties.requestHeaders) n++; return n; });
  o.rpUriT = call(function () { return typeof requestProperties.requestUri + "|" + clean(requestProperties.requestUri); });
  o.cp = call(function () { return typeof clientProperties + "|" + Object.prototype.toString.call(clientProperties) + "|" + clientProperties.size(); });
  o.cpKeys = call(function () { var k = []; for (var x in clientProperties) k.push(x); k.sort(); return k.join(","); });
  o.cpId = call(function () { var c = clientProperties.clientId; return typeof c + "|" + c; });
  o.cpGrants = call(function () { return clientProperties.allowedGrantTypes; });
  o.cpScopes = call(function () { return clientProperties.allowedScopes; });
  o.cpResp = call(function () { return clientProperties.allowedResponseTypes; });
  o.cpRedirect = call(function () { return clientProperties.redirectUris; });
  o.cpCustom = call(function () { return clientProperties.customProperties; });
  o.realm = call(function () { return realm; });
  o.realmEq = call(function () { return realm === "/alpha"; });
  o.scriptName = call(function () { return scriptName; });
  o.cookieNameT = call(function () { return typeof cookieName; });
  o.loggerName = call(function () { return logger.getName(); });
  o.thisT = call(function () { return typeof this; });
}
function validateAccessTokenScope() {
  out.fn.args = arguments.length;
  body(out.fn);
  if (typeof scopeValidatorHelper !== "undefined") { scopeValidatorHelper.throwInvalidScope(JSON.stringify(out)); }
  throw new org.forgerock.oauth2.core.exceptions.InvalidScopeException(JSON.stringify(out));
  return [];
}
