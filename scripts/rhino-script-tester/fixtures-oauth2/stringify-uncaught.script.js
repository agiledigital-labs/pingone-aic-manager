// D4, the consequence half. `java-class-shutter.script.js` wraps every probe in
// a try/catch, so it answers "does this throw" but not "what does the CLIENT
// see". This one lets the prohibited-class error propagate out of
// `validateAccessTokenScope`, which is what a debug line added to a working
// script would do.
//
// Paired with `control-passthrough.script.js`: same client, same request, same
// two scopes. The control is a 200 carrying both. If this one is not a 200,
// the difference is the uncaught throw and nothing else.
//
// The stringify result is deliberately discarded rather than logged —
// `clientProperties` is a client profile.
function asArray(collection) {
  var out = [];
  var items = collection.toArray();
  for (var i = 0; i < items.length; i++) {
    out.push(String(items[i]));
  }
  return out;
}

function validateAccessTokenScope() {
  logger.error("AICEDIT-D4 uncaught: about to stringify clientProperties");
  var ignored = JSON.stringify(clientProperties);
  logger.error("AICEDIT-D4 uncaught: survived, length=" + ignored.length);
  return asArray(requestedScopes);
}
