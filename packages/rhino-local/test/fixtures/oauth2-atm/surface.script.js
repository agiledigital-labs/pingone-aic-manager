// Access-token modification conformance probe: the binding surface. Runs
// unchanged on AIC (both engines) and in the emulator; the result rides out in
// the token's `aicprobe` claim. Reads only what docs/api/12 records as
// measured, so the emulator raises no harness fault. Prints no token id, host
// or header value, and never calls identity.getAttributes().
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
var names = ["accessToken", "identity", "session", "scopes", "requestProperties", "clientProperties", "logger",
  "httpClient", "systemEnv", "realm", "scriptName", "cookieName", "secrets", "openidm", "utils", "idRepository",
  "emailService", "jwtAssertion", "jwtValidator", "policy", "require", "nodeState", "action", "callbacksBuilder",
  "callbacks", "sharedState", "transientState", "requestHeaders", "requestParameters", "requestCookies",
  "existingSession", "cacheManager", "journey", "samlApplication", "oauthApplication", "locales",
  "resumedFromSuspend", "outcome"];
out.typeof = {};
for (var i = 0; i < names.length; i++) {
  try { out.typeof[names[i]] = eval("typeof " + names[i]); } catch (e) { out.typeof[names[i]] = "throw:" + clean(e); }
}
out.realm = call(function () { return realm; });
out.sessionRead = call(function () { return session; });
var stable = ["getRealm", "getResourceOwnerId", "getClientId", "getGrantType", "getTokenType", "getTokenName",
  "isExpired", "getAudience", "getScope", "getCustomFields", "getNonce", "getAct", "getMayAct", "getPermissions",
  "getClaims", "getAuthLevel", "getConfirmationKey"];
out.at = {};
for (var g = 0; g < stable.length; g++) {
  (function (m) { out.at[m] = call(function () { return accessToken[m](); }); })(stable[g]);
}
out.clientIdStrict = accessToken.getClientId() === "zzemuC-atm";
out.clientIdLoose = accessToken.getClientId() == "zzemuC-atm";
out.volatileTypes = typeof accessToken.getAuthTimeSeconds() + "," + typeof accessToken.getExpiryTime() + "," +
  typeof accessToken.getTokenId() + "," + typeof accessToken.getAuditTrackingId() + "," + typeof accessToken.getAuthGrantId();
out.expiryMinusAuth = accessToken.getExpiryTime() - accessToken.getAuthTimeSeconds() * 1000;
out.fieldUnset = call(function () { return accessToken.getField("zzunset"); });
out.customFieldKeys = call(function () { var m = accessToken.getCustomFields(); var k = []; for (var x in m) k.push(x); k.sort(); return k.join(","); });
out.missingMethods = call(function () { return [typeof accessToken.getType, typeof accessToken.getValue, typeof accessToken.getExpiresIn].join(","); });
out.resourceOwner = nextGen ? "skipped" : call(function () { return accessToken.getResourceOwner(); });
if (nextGen) {
  out.idName = call(function () { return identity.getName(); });
  out.idUniversal = call(function () { return identity.getUniversalId(); });
  out.idExists = call(function () { return identity.exists(); });
  out.idAttrValues = call(function () { return identity.getAttributeValues("com.forgerock.openam.oauth2provider.clientType"); });
  out.idAttrValuesMissing = call(function () { return identity.getAttributeValues("zzNoSuchAttr"); });
  out.idAttr = call(function () { return identity.getAttribute("com.forgerock.openam.oauth2provider.clientType"); });
  out.idIsExists = call(function () { return identity.isExists(); });
} else {
  out.identity = call(function () { return identity; });
  out.idName = call(function () { return identity.getName(); });
  out.idUniversal = call(function () { return identity.getUniversalId(); });
  out.idIsExists = call(function () { return identity.isExists(); });
  out.idAttr = call(function () { return identity.getAttribute("com.forgerock.openam.oauth2provider.clientType"); });
  out.idExists = call(function () { return identity.exists(); });
  out.idAttrValues = call(function () { return identity.getAttributeValues("com.forgerock.openam.oauth2provider.clientType"); });
}
out.rpKeys = call(function () { var k = []; for (var x in requestProperties) k.push(x); k.sort(); return k.join(","); });
out.rpSize = call(function () { return requestProperties.size(); });
out.rpRealm = call(function () { return requestProperties.realm; });
out.rpUriPath = call(function () { return String(requestProperties.requestUri).replace(/^https?:\/\/[^\/]+/, ""); });
out.rpParams = call(function () { return requestProperties.requestParams; });
out.rpParamKeys = call(function () { var k = []; for (var x in requestProperties.requestParams) k.push(x); k.sort(); return k.join(","); });
out.rpGrant = call(function () { return requestProperties.requestParams.grant_type; });
out.rpGrant0 = call(function () { return requestProperties.requestParams.grant_type[0]; });
out.rpGrantGet = call(function () { return requestProperties.requestParams.get("grant_type"); });
out.rpHeaderLower = call(function () { return requestProperties.requestHeaders["content-type"]; });
out.rpHeaderTitle = call(function () { return requestProperties.requestHeaders["Content-Type"]; });
out.cpKeys = call(function () { var k = []; for (var x in clientProperties) k.push(x); k.sort(); return k.join(","); });
out.cpSize = call(function () { return clientProperties.size(); });
out.cpClientId = call(function () { return clientProperties.clientId; });
out.cpGet = call(function () { return clientProperties.get("clientId"); });
out.cpGrants = call(function () { return clientProperties.allowedGrantTypes; });
out.cpScopes = call(function () { return clientProperties.allowedScopes; });
out.cpRTSize = call(function () { return clientProperties.allowedResponseTypes.size(); });
out.cpCustom = call(function () { return clientProperties.customProperties; });
out.scopes = call(function () { return scopes; });
out.scopesContains = call(function () { return scopes.contains("zzemuc-probe"); });
out.scopes0 = call(function () { return scopes[0]; });
accessToken.setField("aicprobe", JSON.stringify(out));
