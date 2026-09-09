// D3: is `identity` usable on this grant?
//
// `13-script-contexts.md` lists `identity` for OAUTH2_VALIDATE_SCOPE_NEXT_GEN
// with no qualification, which reads as "uniform across every grant that uses
// this context". The claim under test is that it is not: the binding exists on
// the token-exchange grant but its inner `AMIdentity` is null, so the first
// method call throws and the whole token request fails.
//
// Reported through the log rather than the response because a validate-scope
// script has no other return channel. It must NOT throw — a fixture that dies
// proves only that something went wrong, and the interesting distinction is
// between "absent", "present but empty" and "usable".
function asArray(collection) {
  var out = [];
  var items = collection.toArray();
  for (var i = 0; i < items.length; i++) {
    out.push(String(items[i]));
  }
  return out;
}

function describeIdentity() {
  if (typeof identity === "undefined") {
    return "undefined";
  }
  if (identity === null) {
    return "null";
  }
  // Deliberately not JSON.stringify: on a Java-backed binding that throws a
  // prohibited-class error and takes the whole token request with it (D4).
  try {
    return "usable name=" + String(identity.getName());
  } catch (e) {
    return "bound but getName() threw: " + String(e);
  }
}

function validateAccessTokenScope() {
  var grant = "<unknown>";
  try {
    grant = String(requestProperties.requestParams.grant_type.toArray()[0]);
  } catch (e) {
    grant = "<grant_type unreadable: " + String(e) + ">";
  }
  logger.error("AICEDIT-D3 grant=" + grant + " identity=" + describeIdentity());
  return asArray(requestedScopes);
}
