// Next-gen OIDC claims (measured 2026-11-10).

// `session` was unbound on a password grant (authorization_code is unmeasured), so
// it is `| undefined`: probe it with `typeof`. A bare read is a documented limitation.
if (typeof session !== "undefined" && session !== undefined) {
  logger.info("session bound");
}

// `claims` is a copy of AM's { sub, subname }; values are JS strings.
if (claims.sub === identity.getName() && claims.containsKey("subname")) {
  claims.put("email", "a@example.com");
  logger.info("sub {}", String(claims.get("sub")));
}
if (scopes.contains("profile") && scopes.length > 0) {
  logger.info("scope {}", String(scopes[0]));
}

// The result is the completion value: `{ values, compositeScopes? }`.
/** @type {OidcClaimsResult} */
var result = {
  values: { sub: claims.sub, n: 1 },
  compositeScopes: { profile: ["name"] },
};
/** @type {OidcClaimsResult} */
var minimal = { values: {} };
logger.info("{} {}", String(result), String(minimal));
// toArray() is measured (not a JS array), so only length and [i] are declared.
logger.info("{}", String(scopes.toArray().length));
