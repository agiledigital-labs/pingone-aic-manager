// D1 claim 1: what does scopeValidatorHelper expose? The published binding
// metadata is `{"name":"scopeValidatorHelper","javaScriptType":"unknown"}` —
// no javaClass, no elements — so enumeration is the only route.
function asArray(collection) {
  var out = [];
  var items = collection.toArray();
  for (var i = 0; i < items.length; i++) {
    out.push(String(items[i]));
  }
  return out;
}

function validateAccessTokenScope() {
  var members = [];
  for (var k in scopeValidatorHelper) {
    members.push(k + ":" + typeof scopeValidatorHelper[k]);
  }
  members.sort();
  logger.error("AICEDIT-D1 members=" + members.join(","));
  return asArray(requestedScopes);
}
