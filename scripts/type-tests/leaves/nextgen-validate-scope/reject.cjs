// The scope sets are Java lists: none of Array.prototype exists on them.
// Rows marked "policy:" are not runtime failures; they pin what is left undeclared.
requestedScopes.map(function (s) { return s; }); // expect: TS2339 — Cannot find function map
requestedScopes.join(","); // expect: TS2339 — Cannot find function join
requestedScopes.iterator(); // expect: TS2339 — class shutter (ArrayList$Itr)
allowedScopes.filter(function () { return true; }); // expect: TS2339 — no filter
defaultScopes.includes("a"); // expect: TS2339 — contains(), not includes()
scopeValidatorHelper.throwInvalidScope(); // expect: TS2554 — no argument is a 500
scopeValidatorHelper.reject("x"); // expect: TS2339 — the helper has two measured members
clientProperties.allowedScopes; // expect: TS4111 — the class shutter on every read
// policy: not runtime failures but unmeasured members, left undeclared.
availableScopes.add("a"); // expect: TS2339 — policy: add() was called on requestedScopes only
requestedScopes.toArray().includes("a"); // expect: TS2339 — policy: toArray() is only measured as "not a JS array"
// policy: `[].concat` was measured on a decision node's request value lists,
// not on an OAuth2 `requestProperties` list. Inside a function because the
// `throwInvalidScope()` call above returns `never`, so top-level code after it
// is unreachable and its `null` check would not narrow.
function concatRequestParam() {
  var scopeParamList = requestProperties.requestParams.get("scope");
  if (scopeParamList !== null) {
    logger.info("{}", [].concat(scopeParamList).length); // expect: TS2769 — policy: requestProperties concat is unmeasured
  }
}
concatRequestParam();
