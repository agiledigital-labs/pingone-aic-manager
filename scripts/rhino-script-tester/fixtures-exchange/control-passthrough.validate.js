// CONTROL. Returns exactly what was requested, so a 200 carrying the scope
// proves the client, the override wiring and the entry point all work on
// whichever grant the case used. Every other case here is only meaningful
// against this baseline.
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
  logger.error("AICEDIT-D3 control grant reached validate-scope, requested=" + requested.join(","));
  return requested;
}
