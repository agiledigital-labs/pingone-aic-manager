// Probe: verify the reports<->manager AM-name swap (docs/api/14).
// Setup (IDM): user A (probe-rpt-a) has manager = user B (probe-mgr-b);
// therefore B.reports = [A]. Per the Ping mapping, IDM `manager` -> AM
// `fr-idm-managed-user-manager` and IDM `reports` -> AM `manager`. So:
//   A: fr-idm-managed-user-manager size>0, manager size 0
//   B: manager size>0, fr-idm-managed-user-manager size 0
// Counts only; no _ref values emitted. Safe to delete.
var A = "d0b1a263-23c9-4cc7-b183-95c02f26a6cc"; // probe-rpt-a (has a manager)
var B = "7f201a38-a236-40c5-bba0-e5f4da11bbaa"; // probe-mgr-b (has a report)

function emit(payload) {
  if (callbacks.isEmpty()) {
    callbacksBuilder.hiddenValueCallback("result", JSON.stringify(payload));
  }
  outcome = payload.ok ? "ok" : "error";
}

function sz(id, name) {
  try {
    var vals = idRepository.getIdentity(id).getAttributeValues(name);
    if (vals === null || vals === undefined) { return 0; }
    if (typeof vals.size === "function") { return vals.size(); }
    if (typeof vals.length === "number") { return vals.length; }
    return -1;
  } catch (e) { return "err: " + String(e); }
}

try {
  emit({
    ok: true,
    feature: "identity-manager-swap",
    value: JSON.stringify({
      A_frManager: sz(A, "fr-idm-managed-user-manager"),
      A_manager: sz(A, "manager"),
      B_frManager: sz(B, "fr-idm-managed-user-manager"),
      B_manager: sz(B, "manager")
    })
  });
} catch (e) {
  emit({ ok: false, feature: "identity-manager-swap", error: String(e) });
}
