#!/usr/bin/env bash
# experiment-journey-page.sh
#
# Question this answers:
#   What does a Page node containing UsernameCollector + PasswordCollector + a
#   next-gen Scripted Decision child send and do: callback order, whether the
#   scripted child's callbacks merge into the page, how the child's goTo maps to
#   the page's outcome, and which state keys the collectors write (shared or
#   transient) as seen by the next node.
#
# Method:
#   Variant "b" and variant "a" are identical except the scripted child's
#   resume pass calls goTo("b") or goTo("a"); the page's outcomes are wired to
#   two dump nodes tagged B and A, so the dump that answers names the outcome
#   (each variant is the other's control). The dump node emits a state snapshot
#   on its first pass and again after its own callback round trip: a key that
#   disappears on pass 2 was transient. Variant "nocb" has a scripted child
#   that sends no callbacks. Variant "order" places a single-outcome scripted
#   child between the collectors. A page whose non-last child has two outcomes
#   is also submitted, to record the config rejection.
#
# Every object is named zzemu-* and deleted on exit; the deletion report is
# printed. Output passes through check-sensitive-metadata.sh --redact.

set -uo pipefail
# shellcheck disable=SC1091  # the prelude is checked on its own
. "$(dirname "${BASH_SOURCE[0]}")/experiment-journey-prelude.sh"

python3 - <<'PY' 2>&1 | "$REDACT" --redact
import json
from experiment_journey_lib import (Fixture, authenticate, show, fill, dump_source,
                                    log, SUCCESS, call, NODES, list_outcomes)

CHILD = r"""
if (callbacks.isEmpty()) {
  nodeState.putShared("zzScriptPass1", "yes");
  if (EMIT) {
    callbacksBuilder.textOutputCallback(0, "zzemu script text");
    callbacksBuilder.nameCallback("zzemu script prompt");
  } else {
    var n0 = callbacks.getNameCallbacks();
    nodeState.putShared("zzScriptSawNamesPass1", String(n0.size()));
    nodeState.putShared("zzScriptSawUsername", String(nodeState.get("username")));
    action.goTo(GOTO);
  }
} else {
  var n = callbacks.getNameCallbacks();
  var seen = [];
  for (var i = 0; i < n.size(); i++) { seen.push(String(n.get(i))); }
  nodeState.putShared("zzScriptSawNames", seen.join("|"));
  nodeState.putShared("zzScriptSawUsername", String(nodeState.get("username")));
  var pw = nodeState.get("password");
  nodeState.putShared("zzScriptSawPasswordLen", pw === null ? "null" : String(String(pw).length));
  action.goTo(GOTO);
}
"""
KEYS = ["username", "password", "zzScriptPass1", "zzScriptSawNames", "zzScriptSawNamesPass1",
        "zzScriptSawUsername", "zzScriptSawPasswordLen"]
PW = "zzemu-pw-xyz123"


def build(fx, tag, goto, emit, order="last", wire=None):
    child_src = CHILD.replace("GOTO", json.dumps(goto)).replace("EMIT", "true" if emit else "false")
    outcomes = ["true"] if order == "middle" else ["a", "b"]
    s = fx.scripted(f"{tag}-child", child_src, outcomes)
    u = fx.node("UsernameCollectorNode", {})
    p = fx.node("PasswordCollectorNode", {})
    kids = {"u": {"_id": u, "nodeType": "UsernameCollectorNode", "displayName": "zzemu user"},
            "p": {"_id": p, "nodeType": "PasswordCollectorNode", "displayName": "zzemu pw"},
            "s": {"_id": s, "nodeType": "ScriptedDecisionNode", "displayName": "zzemu script"}}
    seq = "usp" if order == "middle" else "ups"
    cfg = {"nodes": [kids[c] for c in seq], "pageHeader": {}, "pageDescription": {}}
    pg = fx.node("PageNode", cfg)
    listed = list_outcomes("PageNode", {"nodes": cfg["nodes"]})
    log(f"--- variant {tag}: page listOutcomes = {listed}")
    nodes = {pg: ("PageNode", "page", {})}
    for oc in (wire or listed):
        d = fx.scripted(f"{tag}-dump{oc}", dump_source(KEYS, oc.upper()), ["true"])
        nodes[d] = ("ScriptedDecisionNode", f"dump {oc}", {"true": SUCCESS})
        nodes[pg][2][oc] = d
    return fx.tree(tag, pg, nodes)


def drive(tree, label, emit):
    r1 = authenticate(tree)
    show(f"{label} step1 (empty POST)", r1)
    body = r1[1]
    vals = {}
    for i, c in enumerate(body.get("callbacks", [])):
        prompt = {o["name"]: o.get("value") for o in c.get("output", [])}.get("prompt")
        if c["type"] == "NameCallback":
            vals[i] = "zzemu-script-answer" if prompt == "zzemu script prompt" else "zzemu-page-user"
        elif c["type"] == "PasswordCallback":
            vals[i] = PW
    r2 = authenticate(tree, fill(body, vals))
    show(f"{label} step2 (page submitted)", r2)
    if r2[0] == 200 and "callbacks" in r2[1]:
        r3 = authenticate(tree, r2[1])
        show(f"{label} step3 (dump resumed)", r3)


fx = Fixture("page")
try:
    # Config rejection: two-outcome child not last.
    s = fx.scripted("bad-child", "action.goTo('a');", ["a", "b"])
    u = fx.node("UsernameCollectorNode", {})
    st, b, _ = call("PUT", f"{NODES}/PageNode/00000000-0000-4000-8000-00000000zz01".replace("zz", "00"),
                    {"nodes": [{"_id": s, "nodeType": "ScriptedDecisionNode", "displayName": "x"},
                               {"_id": u, "nodeType": "UsernameCollectorNode", "displayName": "y"}]})
    log("--- page with two-outcome child first:", st, json.dumps(b))
    if st in (200, 201):
        fx.track("node", f"{NODES}/PageNode/00000000-0000-4000-8000-000000000001")
    # "unwired" is "order" with the tree wiring the scripted child's outcome
    # ("true") instead of the page's listed one: the control for the 500.
    for tag, goto, emit, order, wire in [
            ("b", "b", True, "last", None), ("a", "a", True, "last", None),
            ("nocb", "b", False, "last", None), ("order", "true", True, "middle", None),
            ("unwired", "true", True, "middle", ["true"])]:
        tree = build(fx, tag, goto, emit, order, wire)
        drive(tree, f"variant {tag}", emit)
finally:
    log("--- cleanup")
    for line in fx.cleanup():
        log("   ", line)
PY
