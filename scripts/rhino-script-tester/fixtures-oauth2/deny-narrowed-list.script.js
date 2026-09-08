// D1 claim 2c: the DANGEROUS denial, and the whole reason this probe exists.
// Returning the requested list minus the denied scope is the intuitive reading
// of a function called "validate scope". If the claim holds, the client gets
// 200 with a token, and the scope is simply absent from the response.
function asArray(collection) {
  var out = [];
  var items = collection.toArray();
  for (var i = 0; i < items.length; i++) {
    out.push(String(items[i]));
  }
  return out;
}

function validateAccessTokenScope() {
  var requested = asArray(requestedScopes);
  var kept = [];
  for (var i = 0; i < requested.length; i++) {
    if (requested[i] !== "aicedit-probe") {
      kept.push(requested[i]);
    }
  }
  logger.error("AICEDIT-D1 narrowed=" + requested.join(",") + " -> " + kept.join(","));
  return kept;
}
