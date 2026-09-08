// CONTROL. Returns exactly what was requested, so a 200 carrying the scope
// proves the client, the override wiring and the entry point all work. Every
// denial fixture below is only meaningful against this baseline.
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
  logger.error("AICEDIT-D1 control requested=" + requested.join(","));
  return requested;
}
