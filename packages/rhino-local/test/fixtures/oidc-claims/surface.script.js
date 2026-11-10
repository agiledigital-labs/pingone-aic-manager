// OIDC claims conformance probe: the binding surface. Runs unchanged on AIC
// (both engines) and in the emulator; the record rides out in the `aicprobe`
// claim. Never prints a request parameter or header value (the token request
// carries the password), and never calls identity.getAttributes().
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
var names = ["claims", "claimObjects", "requestedClaims", "requestedTypedClaims", "claimsLocales", "identity",
  "session", "scopes", "requestProperties", "clientProperties", "logger", "httpClient", "systemEnv", "realm",
  "scriptName", "cookieName", "secrets", "openidm", "utils", "idRepository", "emailService", "jwtAssertion",
  "jwtValidator", "policy", "require", "accessToken", "nodeState", "action", "callbacksBuilder", "callbacks",
  "sharedState", "transientState", "requestHeaders", "requestParameters", "existingSession", "outcome",
  "JavaImporter", "Packages"];
out.typeof = {};
for (var i = 0; i < names.length; i++) {
  try { out.typeof[names[i]] = eval("typeof " + names[i]); } catch (e) { out.typeof[names[i]] = "throw:" + clean(e); }
}
out.where = call(function () { return String(requestProperties.requestUri).replace(/^https?:\/\/[^\/]+/, ""); });
out.realm = call(function () { return realm; });
out.scriptName = call(function () { return scriptName; });
out.session = call(function () { return session; });
out.claims = call(function () { return claims; });
out.claimsKeys = keys(function () { return claims; });
out.claimsSub = call(function () { return claims.sub; });
out.claimsGet = call(function () { return claims.get("sub"); });
out.requestedClaims = call(function () { return requestedClaims; });
out.claimObjects = call(function () { return claimObjects; });
out.requestedTypedClaims = call(function () { return requestedTypedClaims; });
out.claimsLocales = call(function () { return claimsLocales; });
out.scopes = call(function () { return scopes; });
out.scopesContains = call(function () { return scopes.contains("zzemuc2-probe"); });
out.scopes0 = call(function () { return scopes[0]; });
out.scopesToArray = call(function () { return scopes.toArray().length; });
out.idName = call(function () { return identity.getName(); });
out.idUniversal = call(function () { return identity.getUniversalId(); });
out.idExists = call(function () { return identity.exists(); });
out.idIsExists = call(function () { return identity.isExists(); });
out.idAttr = call(function () { return identity.getAttribute("mail"); });
out.idAttrMissing = call(function () { return identity.getAttribute("zzNoSuchAttr"); });
out.idAttrValues = call(function () { return identity.getAttributeValues("mail"); });
out.idAttrValuesMissing = call(function () { return identity.getAttributeValues("zzNoSuchAttr"); });
out.idAttrValues0 = call(function () { return identity.getAttributeValues("givenName")[0]; });
out.idAttrValues0Type = call(function () { return typeof identity.getAttributeValues("givenName")[0]; });
out.rpKeys = keys(function () { return requestProperties; });
out.rpSize = call(function () { return requestProperties.size(); });
out.rpRealm = call(function () { return requestProperties.realm; });
out.rpParamKeys = keys(function () { return requestProperties.requestParams; });
out.rpGrant = call(function () { return requestProperties.requestParams.grant_type; });
out.rpGrant0Type = call(function () { return typeof requestProperties.requestParams.grant_type[0]; });
out.rpGrantGet = call(function () { return requestProperties.requestParams.get("grant_type"); });
out.rpScope = call(function () { return requestProperties.requestParams.scope; });
out.rpHeaderKeyCount = call(function () { var n = 0; for (var x in requestProperties.requestHeaders) n++; return n; });
out.rpHeaderLowerType = call(function () { return typeof requestProperties.requestHeaders["content-type"]; });
out.cpKeys = keys(function () { return clientProperties; });
out.cpSize = call(function () { return clientProperties.size(); });
out.cpClientId = call(function () { return clientProperties.clientId; });
out.cpClientIdStrict = call(function () { return clientProperties.clientId === "zzemuC2-oidc"; });
out.cpGrants = call(function () { return clientProperties.allowedGrantTypes; });
out.cpScopes = call(function () { return clientProperties.allowedScopes; });
out.cpRTSize = call(function () { return clientProperties.allowedResponseTypes.size(); });
out.cpCustom = call(function () { return clientProperties.customProperties; });
out.cpRedirect = call(function () { return clientProperties.redirectUris; });
var probe = JSON.stringify(out);
if (nextGen) {
  ({ values: { aicprobe: probe }, compositeScopes: {} });
} else {
  var m = new java.util.LinkedHashMap();
  m.put("aicprobe", probe);
  new org.forgerock.oauth2.core.UserInfoClaims(m, new java.util.LinkedHashMap());
}
