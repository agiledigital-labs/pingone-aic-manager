scopes.map(function (s) { return s; }); // expect: TS2339 — a Java list has no map
clientProperties.allowedGrantTypes; // expect: TS4111 — the class shutter on every read
/** @type {OidcClaimsResult} */ var noValues = { compositeScopes: {} }; // expect: TS2741 — a result with no `values` emits no claims
/** @type {OidcClaimsResult} */ var badValues = { values: "x" }; // expect: TS2322 — values is a map
claims.put("a"); // expect: TS2554 — put takes a key and a value
String(noValues) + String(badValues);
// policy: not a known runtime failure; add() is an unmeasured member here, left undeclared.
scopes.add("x"); // expect: TS2339 — policy: add() was not measured on OIDC claims scopes
