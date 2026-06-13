// Probe: enumerate what the ScriptedIdentity actually exposes for user A
// (probe-rpt-a, who has manager set in IDM) — to see whether relationship
// attrs surface at all and under what method/name. Reports method availability
// and, where a lister exists, the attribute NAME set (names are schema, not
// PII). Tries a handful of manager-name candidates by count. Safe to delete.
var A = "d0b1a263-23c9-4cc7-b183-95c02f26a6cc";

function emit(payload) {
  if (callbacks.isEmpty()) {
    callbacksBuilder.hiddenValueCallback("result", JSON.stringify(payload));
  }
  outcome = payload.ok ? "ok" : "error";
}

function names(coll) {
  // coll is a java Map or Set of names; return a sorted JS array via toArray.
  try {
    if (coll && typeof coll.keySet === "function") { coll = coll.keySet(); }
    if (coll && typeof coll.toArray === "function") {
      var a = coll.toArray(); var out = [];
      for (var i = 0; i < a.length; i++) { out.push(String(a[i])); }
      out.sort(); return out;
    }
    return "no-toArray:" + String(coll);
  } catch (e) { return "err:" + String(e); }
}

try {
  var id = idRepository.getIdentity(A);
  var info = {};
  info.methods = {
    getAttributeValues: typeof id.getAttributeValues,
    getAttributes: typeof id.getAttributes,
    getAttributeNames: typeof id.getAttributeNames,
    asMap: typeof id.asMap
  };
  // Try to list all attribute names via whichever lister exists.
  if (typeof id.getAttributes === "function") {
    try { info.getAttributes = names(id.getAttributes()); }
    catch (e) { info.getAttributes = "err:" + String(e); }
  }
  // Candidate manager-ish AM names by count.
  var cands = ["manager", "fr-idm-managed-user-manager", "reports",
               "fr-idm-managed-user-reports", "dn", "entryDN"];
  info.counts = {};
  for (var i = 0; i < cands.length; i++) {
    try {
      var v = id.getAttributeValues(cands[i]);
      info.counts[cands[i]] = (v && typeof v.size === "function") ? v.size() : -1;
    } catch (e) { info.counts[cands[i]] = "err"; }
  }
  emit({ ok: true, feature: "identity-enum-attrs", value: JSON.stringify(info) });
} catch (e) {
  emit({ ok: false, feature: "identity-enum-attrs", error: String(e) });
}
