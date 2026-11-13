// Each marked line is a runtime failure on next-gen token modification.
accessToken.getAuthLevel() + 1; // expect: TS2531 — null on a token with no value
accessToken.getNonce().length; // expect: TS2531 — null on a token with no value
accessToken.getAct().toString(); // expect: TS2531 — JS null when there is no act claim
scopes.map(function (s) { return s; }); // expect: TS2339 — a Java list has no map
scopes.join(","); // expect: TS2339 — a Java list has no join
clientProperties.allowedScopes; // expect: TS4111 — the class shutter on every read
clientProperties["allowedGrantTypes"].contains("x"); // expect: TS2571 — unknown, never a readable set
requestProperties.requestHeaders["x"]?.includes("a"); // expect: TS2339 — the value list has no includes
