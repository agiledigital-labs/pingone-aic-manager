// Next-gen access-token modification, as measured on the sandbox 2026-11-10
// (docs/api/12-script-bindings-matrix.md).

// `session` is not bound at all, and is not declared: neither a bare read nor a
// `typeof` probe compiles (see reject.cjs).

// Getters answer JS null on a token with no value, so each read is guarded.
var level = accessToken.getAuthLevel();
if (level !== null) {
  accessToken.setAuthLevel(level + 1);
}
var nonce = accessToken.getNonce();
if (nonce) {
  accessToken.setNonce(nonce);
}
var act = accessToken.getAct();
if (act !== null) {
  logger.info("act present");
}

// The JSON string form is the measured call.
accessToken.addExtraJsonData("extra", '{"a":1}');

// `scopes` is a Java list: length, [i], size() and contains() are measured, and
// the token copies what setScope is given, from a JS array or from the list.
if (scopes.contains("openid") && scopes.length > 0 && scopes.size() > 0) {
  accessToken.setScope(scopes);
}
accessToken.setScope(["openid"]);
logger.info("first scope {}", String(scopes[0]));

// Request values are lists of JS strings, so a strict comparison is a real one.
var grant = requestProperties.requestParams["grant_type"];
if (grant && grant.length > 0 && grant[0] === "client_credentials") {
  logger.info("client credentials");
}
logger.info("{} {}", requestProperties.requestUri, clientProperties.clientId);
