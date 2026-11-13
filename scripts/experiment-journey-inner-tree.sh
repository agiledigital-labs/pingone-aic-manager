#!/usr/bin/env bash
# experiment-journey-inner-tree.sh
#
# Question this answers:
#   What are an Inner Tree Evaluator node's outcomes; is shared and transient
#   state written before it visible inside the inner tree, and are keys written
#   inside visible after it; what does journey.name() / innerJourney() return
#   inside versus outside?
#
# Method:
#   Outer tree: a script writes zzOuterShared (shared) and zzOuterTransient
#   (transient) and records journey.*; then InnerTreeEvaluatorNode; its
#   outcomes go to dump nodes T and F. The inner tree's one script records
#   what it can read, writes zzInnerShared / zzInnerTransient, records
#   journey.*; a second inner script reads the transient key back (the control
#   that transient flows inside the inner tree) and goes to the inner success
#   node ("ok" variant) or the inner failure node ("fail" variant, the control
#   for the outcome mapping). The
#   dump node reports on pass 1 and again after its own callback round trip, so
#   transient keys show as present then absent. listOutcomes is read for the
#   evaluator with displayErrorOutcome false and true, and the inner tree
#   (innerTreeOnly=true) is also invoked directly.
#
# Every object is named zzemu-* and deleted on exit.

set -uo pipefail
# shellcheck disable=SC1091  # the prelude is checked on its own
. "$(dirname "${BASH_SOURCE[0]}")/experiment-journey-prelude.sh"

python3 - <<'PY' 2>&1 | "$REDACT" --redact
import json
from experiment_journey_lib import (Fixture, authenticate, show, dump_source, log,
                                    SUCCESS, FAILURE, list_outcomes)

JOURNEY = r"""
function j() {
  var r = {};
  try { r.name = String(journey.name()); } catch (e) { r.name = "throws:" + e; }
  try { r.inner = String(journey.innerJourney()); } catch (e) { r.inner = "throws:" + e; }
  try { r.mustRun = String(journey.mustRun()); } catch (e) { r.mustRun = "throws:" + e; }
  try { r.identityResource = String(journey.identityResource()); } catch (e) { r.identityResource = "throws:" + e; }
  return JSON.stringify(r);
}
"""
OUTER = JOURNEY + r"""
nodeState.putShared("zzOuterShared", "os");
nodeState.putTransient("zzOuterTransient", "ot");
nodeState.putShared("zzOuterJourney", j());
action.goTo("true");
"""
INNER = JOURNEY + r"""
nodeState.putShared("zzInnerSawShared", String(nodeState.get("zzOuterShared")));
nodeState.putShared("zzInnerSawTransient", String(nodeState.get("zzOuterTransient")));
nodeState.putShared("zzInnerJourney", j());
nodeState.putShared("zzInnerShared", "is");
nodeState.putTransient("zzInnerTransient", "it");
action.goTo("true");
"""
# Second inner node: the control that transient state does flow between nodes
# inside the inner tree, so a null across the tree boundary is not a binding
# that never reads transient.
INNER2 = r"""
nodeState.putShared("zzInner2SawTransient", String(nodeState.get("zzInnerTransient")));
action.goTo(GOTO);
"""
KEYS = ["zzOuterShared", "zzOuterTransient", "zzOuterJourney", "zzInnerSawShared",
        "zzInnerSawTransient", "zzInnerJourney", "zzInnerShared", "zzInnerTransient",
        "zzInner2SawTransient"]

fx = Fixture("inner")
try:
    for flag in (False, True):
        log(f"--- listOutcomes InnerTreeEvaluatorNode displayErrorOutcome={flag}:",
            list_outcomes("InnerTreeEvaluatorNode", {"tree": "x", "displayErrorOutcome": flag}))
    for tag, goto in [("ok", "true"), ("fail", "false")]:
        si = fx.scripted(f"{tag}-in", INNER, ["true"])
        si2 = fx.scripted(f"{tag}-in2", INNER2.replace("GOTO", json.dumps(goto)), ["true", "false"])
        inner = fx.tree(f"{tag}-in", si, {
            si: ("ScriptedDecisionNode", "inner", {"true": si2}),
            si2: ("ScriptedDecisionNode", "inner 2", {"true": SUCCESS, "false": FAILURE})},
            inner_only=True)
        so = fx.scripted(f"{tag}-out", OUTER, ["true"])
        it = fx.node("InnerTreeEvaluatorNode", {"tree": inner, "displayErrorOutcome": False})
        dt = fx.scripted(f"{tag}-dT", dump_source(KEYS, "T"), ["true"])
        df = fx.scripted(f"{tag}-dF", dump_source(KEYS, "F"), ["true"])
        outer = fx.tree(f"{tag}-out", so, {
            so: ("ScriptedDecisionNode", "outer", {"true": it}),
            it: ("InnerTreeEvaluatorNode", "inner tree", {"true": dt, "false": df}),
            dt: ("ScriptedDecisionNode", "dump T", {"true": SUCCESS}),
            df: ("ScriptedDecisionNode", "dump F", {"true": SUCCESS})})
        r1 = authenticate(outer)
        show(f"variant {tag} step1", r1)
        if r1[0] == 200 and "callbacks" in r1[1]:
            show(f"variant {tag} step2 (dump resumed)", authenticate(outer, r1[1]))
        if tag == "ok":
            show("inner tree (innerTreeOnly=true) invoked directly", authenticate(inner))
finally:
    log("--- cleanup")
    for line in fx.cleanup():
        log("   ", line)
PY
