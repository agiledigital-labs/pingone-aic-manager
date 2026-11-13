// Each marked line is a runtime failure on next-gen token modification.
accessToken.getAuthLevel() + 1; // expect: TS2531 — null on a token with no value
accessToken.getNonce().length; // expect: TS2531 — null on a token with no value
accessToken.getAct().toString(); // expect: TS2531 — JS null when there is no act claim
scopes.map(function (s) { return s; }); // expect: TS2339 — a Java list has no map
scopes.join(","); // expect: TS2339 — a Java list has no join
clientProperties.allowedScopes; // expect: TS4111 — the class shutter on every read
clientProperties["allowedGrantTypes"].contains("x"); // expect: TS2571 — unknown, never a readable set
requestProperties.requestHeaders["x"]?.includes("a"); // expect: TS2339 — the value list has no includes
// policy: not a known runtime failure; forEach is an unmeasured member here, left undeclared.
scopes.forEach(function (s) { return s; }); // expect: TS2339 — policy: forEach was not measured on token-modification scopes
[].concat(scopes); // expect: TS2769 — policy: concat was measured on the validate-scope sets, not on token-modification scopes
