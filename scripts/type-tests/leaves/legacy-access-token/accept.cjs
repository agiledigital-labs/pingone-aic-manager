// The legacy access-token-modification leaf: classic Debug method names over
// slf4j formatting. Every call here is correct at runtime.
logger.message("plain message");
logger.error("one {} placeholder", "alpha");
logger.warning("two {} and {}", "alpha", "bravo");

// `\{}` is an escape — no argument for it.
logger.error("escaped \\{} stays literal");
logger.error("escaped \\{} then {} bound", "alpha");

// `\\` is a literal backslash, so the `{}` after it IS a placeholder.
logger.error("double \\\\{} bound", "alpha");

// A trailing throwable is an argument slf4j takes without a `{}`.
try {
  accessToken.setField("x", 1);
} catch (e) {
  logger.error("could not set field", e);
  logger.error("could not set field for {}", "alpha", e);
}

// A widened message cannot be counted, so it stays unchecked.
var built = "dynamic " + String(scopes);
logger.message(built, 1, 2, 3);

// A template literal keeps its type: the visible `{}` is still counted.
logger.error(`realm ${realm} said {}`, "alpha");

if (logger.messageEnabled()) {
  logger.message("guarded {}", realm);
}

// --- the measured legacy access-token-modification surface ------------------
var realmName = String(accessToken.getRealm());
accessToken.setField("aic_realm", realmName);
accessToken.setFields({ aic_batch: 1 });
if (!accessToken.isExpired() && accessToken.getExpiryTime() > 0) {
  accessToken.setField("aic_owner", String(accessToken.getResourceOwnerId()));
}

// `scopes` is a java.util.Set: membership, not indexing.
if (scopes.contains("openid") && scopes.size() > 0) {
  accessToken.setField("aic_openid", true);
}

// `setScope` takes a Set, never a JS array.
var wanted = new java.util.HashSet();
wanted.add("openid");
accessToken.setScope(wanted);

// The legacy AMIdentity spellings.
if (identity.isExists() && identity.isActive()) {
  var mail = identity.getAttribute("mail");
  if (mail && mail.size() > 0) {
    accessToken.setField("aic_mail", String(mail.get(0)));
  }
}

// requestProperties members are properties; the maps inside are Java multimaps.
var grantType = requestProperties.requestParams["grant_type"];
if (grantType && grantType.length) {
  logger.message("token-mod for grant {}", String(grantType[0]));
}
logger.message(
  "client {} in realm {}",
  clientProperties.clientId,
  requestProperties.realm
);
if (clientProperties.allowedScopes.contains("openid")) {
  accessToken.setField("aic_client_allows_openid", true);
}

// `session` is null on every grant measured, so it has to be guarded.
if (session) {
  accessToken.setField("aic_has_session", true);
}

// Nullable getters force the question.
var nonce = accessToken.getNonce();
accessToken.setField("aic_nonce", nonce === null ? "none" : String(nonce));
