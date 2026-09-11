// Corpus copy of scripts/rhino-script-tester/fixtures/identity-resolve-diag.script.js. Do not stub missing bindings.
// Tenant user names and ids replaced with reserved placeholders.

// Diagnostic: find what key idRepository.getIdentity() resolves in a scripted
// decision (the mapping probe got amIdentity==null for a plain userName).
// Tries several candidate keys and reports, per candidate, whether a probe
// attribute call (givenName) succeeds — NO values emitted, just ok/err.
// Safe to delete.
function emit(payload) {
  if (callbacks.isEmpty()) {
    callbacksBuilder.hiddenValueCallback("result", JSON.stringify(payload));
  }
  outcome = payload.ok ? "ok" : "error";
}

var CANDIDATES = [
  "alice",                          // userName / uid
  "00000000-0000-0000-0000-000000000000",        // managed-user uuid
  "amadmin"                                       // known admin (root)
];

function probe(key) {
  try {
    var id = idRepository.getIdentity(key);
    if (id === null || id === undefined) { return "identity-null"; }
    try {
      var vals = id.getAttributeValues("givenName");
      var n = (vals && typeof vals.size === "function") ? vals.size()
            : (vals && typeof vals.length === "number") ? vals.length : -1;
      return "ok givenName-size=" + n;
    } catch (ae) {
      return "attr-error: " + String(ae);
    }
  } catch (ge) {
    return "getIdentity-error: " + String(ge);
  }
}

try {
  var out = {};
  for (var i = 0; i < CANDIDATES.length; i++) { out[CANDIDATES[i]] = probe(CANDIDATES[i]); }
  out["idRepository_typeof"] = typeof idRepository;
  out["nodeState_username"] = (typeof nodeState !== "undefined" && nodeState)
    ? String(nodeState.get("username")) : "no-nodeState";
  emit({ ok: true, feature: "identity-resolve-diag", value: JSON.stringify(out) });
} catch (e) {
  emit({ ok: false, feature: "identity-resolve-diag", error: String(e) });
}
