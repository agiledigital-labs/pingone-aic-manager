// Next-gen OIDC claims (measured 2026-11-10).

// `session` is not bound on a password grant, and is not declared (reject.cjs).

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
