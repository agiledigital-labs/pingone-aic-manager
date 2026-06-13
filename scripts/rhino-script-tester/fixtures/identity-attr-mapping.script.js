// Probe: verify the IDM-property -> AM-attribute mapping for the `identity`
// binding (docs/api/14). For a known sandbox TEST user it calls
// idRepository.getIdentity(user).getAttributeValues(<amName>) for a
// representative set of AM attribute names from the Ping reference mapping and
// emits only the VALUE COUNT per name (never the values — keeps PII out of the
// results/logs). A correct AM name on a populated field returns size>0; a wrong
// name returns 0/empty/error. `fr-idm-custom-attrs` additionally reports its
// top-level keys (custom field NAMES, schema not data).
//
// Safe to delete. Reads attribute counts only. idRepository.getIdentity() in a
// scripted decision resolves by managed-object UUID (fr-idm-uuid), NOT userName
// (verified — see identity-resolve-diag). PROBE_USER is the test account's uuid.
var PROBE_USER = "838e77f6-0999-4afe-8a3f-13a02b28bf37"; // vuthikTestUsername

function emit(payload) {
  if (callbacks.isEmpty()) {
    callbacksBuilder.hiddenValueCallback("result", JSON.stringify(payload));
  }
  outcome = payload.ok ? "ok" : "error";
}

// AM attribute names to probe (from the Ping IDM->AM reference, docs/api/14).
// Includes deliberate WRONG names (frGivenName, givenNameXYZ) as negative
// controls, and the surprising reports/manager swap.
var AM_NAMES = [
  "uid", "cn", "givenName", "sn", "mail", "displayName", "description",
  "telephoneNumber", "l", "st", "co", "postalCode", "inetUserStatus",
  "fr-idm-uuid", "fr-idm-managed-user-manager", "manager",
  "fr-idm-managed-user-roles", "fr-idm-managed-application-member",
  "fr-idm-consentedMapping", "fr-idm-custom-attrs",
  // negative controls (expect 0 / error):
  "frGivenName", "givenNameXYZ"
];

function sizeOf(vals) {
  if (vals === null || vals === undefined) { return 0; }
  if (typeof vals.size === "function") { return vals.size(); }
  if (typeof vals.length === "number") { return vals.length; }
  // Java collection without size(): count by iteration via toArray if present.
  if (typeof vals.toArray === "function") { return vals.toArray().length; }
  return -1; // unknown shape
}

try {
  var identity = idRepository.getIdentity(PROBE_USER);
  var counts = {};
  var customKeys = null;
  for (var i = 0; i < AM_NAMES.length; i++) {
    var name = AM_NAMES[i];
    try {
      var vals = identity.getAttributeValues(name);
      counts[name] = sizeOf(vals);
      if (name === "fr-idm-custom-attrs" && counts[name] > 0) {
        // The single value is a JSON object string; report its keys only.
        try {
          var first;
          if (typeof vals.toArray === "function") {
            first = String(vals.toArray()[0]);
          } else {
            // Java collection toString is "[{...}]"; strip the outer brackets.
            first = String(vals).replace(/^\[/, "").replace(/\]$/, "");
          }
          var obj = JSON.parse(first);
          customKeys = [];
          for (var k in obj) { if (obj.hasOwnProperty(k)) { customKeys.push(k); } }
        } catch (ce) { customKeys = "parse-error: " + String(ce); }
      }
    } catch (ae) {
      counts[name] = "error: " + String(ae);
    }
  }
  emit({
    ok: true,
    feature: "identity-attr-mapping",
    value: JSON.stringify({ user: PROBE_USER, counts: counts, customKeys: customKeys })
  });
} catch (e) {
  emit({ ok: false, feature: "identity-attr-mapping", error: String(e) });
}
