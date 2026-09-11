// Corpus copy of scripts/rhino-script-tester/fixtures/identity-manager-swap.script.js. Do not stub missing bindings.
// Tenant user names and ids replaced with reserved placeholders.

// Probe: verify the reports<->manager AM-name swap (docs/api/14).
// Setup (IDM): user A (alice) has manager = user B (bob);
// therefore B.reports = [A]. Per the Ping mapping, IDM `manager` -> AM
// `fr-idm-managed-user-manager` and IDM `reports` -> AM `manager`. So:
//   A: fr-idm-managed-user-manager size>0, manager size 0
//   B: manager size>0, fr-idm-managed-user-manager size 0
// Counts only; no _ref values emitted. Safe to delete.
var A = "00000000-0000-0000-0000-000000000001"; // alice (has a manager)
var B = "00000000-0000-0000-0000-000000000002"; // bob (has a report)

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
