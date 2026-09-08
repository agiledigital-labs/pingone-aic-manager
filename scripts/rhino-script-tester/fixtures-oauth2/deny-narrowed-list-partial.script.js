// D1 claim 2c, the case that actually discriminates. `deny-narrowed-list`
// narrows to the EMPTY list, and AM refuses that outright with 403 — which is a
// safe failure and tells us nothing about the reported danger. The claim is
// about dropping ONE scope of several, where a token can still be issued: does
// the client then get 200 with the denied scope quietly missing?
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
  logger.error("AICEDIT-D1 partial-narrow=" + requested.join(",") + " -> " + kept.join(","));
  return kept;
}
